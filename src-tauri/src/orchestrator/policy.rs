// Orchestrator Policy Engine
// Security policy for tool/file gating and secret redaction
// See: docs/orchestration_plan.md

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ============================================================================
// Policy Configuration
// ============================================================================

/// Security tier for actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionTier {
    /// Safe: read-only ops, search, list, diff - Auto-allow
    Safe,
    /// Write: file modifications in workspace - Require approval
    Write,
    /// Dangerous: shell, install, web fetch, git push - Blocked by default
    Dangerous,
}

/// Policy decision for an action
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PolicyDecision {
    /// Action is allowed
    Allow,
    /// Action requires user approval
    RequireApproval { reason: String },
    /// Action is denied
    Deny { reason: String },
}

/// Policy engine configuration
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// Workspace root path
    pub workspace_root: PathBuf,
    /// Auto-approve write actions
    pub auto_approve_writes: bool,
    /// Allow dangerous actions (requires explicit enable)
    pub allow_dangerous: bool,
    /// Paths to always exclude from context
    pub excluded_paths: Vec<String>,
    /// Patterns for secret redaction
    pub secret_patterns: Vec<Regex>,
}

impl PolicyConfig {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            auto_approve_writes: false,
            allow_dangerous: false,
            excluded_paths: vec![
                ".env".to_string(),
                ".env.local".to_string(),
                ".env.production".to_string(),
                "*.pem".to_string(),
                "*.key".to_string(),
                "id_rsa".to_string(),
                "id_ed25519".to_string(),
                ".npmrc".to_string(),
                ".pypirc".to_string(),
            ],
            secret_patterns: Self::default_secret_patterns(),
        }
    }

    fn default_secret_patterns() -> Vec<Regex> {
        // Compile patterns for common secret formats
        // Using raw string literals for regex patterns
        let patterns: Vec<&str> = vec![
            // API keys (generic)
            r#"(?i)(api[_-]?key|apikey)['"]?\s*[:=]\s*['"]?[\w-]{20,}"#,
            // Bearer tokens
            r#"(?i)bearer\s+[\w-]+\.[\w-]+\.[\w-]+"#,
            // AWS keys
            r#"(?i)AKIA[0-9A-Z]{16}"#,
            r#"(?i)aws[_-]?secret[_-]?access[_-]?key['"]?\s*[:=]\s*['"]?[\w/+=]{40}"#,
            // GitHub tokens
            r#"ghp_[a-zA-Z0-9]{36}"#,
            r#"gho_[a-zA-Z0-9]{36}"#,
            r#"ghu_[a-zA-Z0-9]{36}"#,
            r#"ghs_[a-zA-Z0-9]{36}"#,
            r#"ghr_[a-zA-Z0-9]{36}"#,
            // Anthropic keys
            r#"sk-ant-api[a-zA-Z0-9\-_]{90,}"#,
            // OpenAI keys
            r#"sk-[a-zA-Z0-9]{48}"#,
            // Private keys
            r#"-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----"#,
            // Generic secrets
            r#"(?i)(password|passwd|pwd|secret)['"]?\s*[:=]\s*['"]?[^\s'"]{8,}"#,
        ];

        patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()
    }
}

// ============================================================================
// Policy Engine
// ============================================================================

/// Security policy engine for orchestrator
pub struct PolicyEngine {
    config: PolicyConfig,
}

impl PolicyEngine {
    pub fn new(config: PolicyConfig) -> Self {
        Self { config }
    }

    /// Classify the tier of a tool action
    pub fn classify_tier(&self, tool: &str, args: &serde_json::Value) -> ActionTier {
        match tool {
            // Tier 0: Safe (read-only)
            "read_file" | "search" | "list" | "diff" | "glob" | "grep" | "view_file"
            | "search_codebase" | "list_files" => ActionTier::Safe,

            // Tier 2: Dangerous
            "shell" | "bash" | "exec" | "run" | "install" | "npm_install" | "pip_install"
            | "web_fetch" | "fetch_url" | "git_push" | "git_commit" | "delete_file" => {
                ActionTier::Dangerous
            }

            // Tier 1: Write (default for unknown tools with file paths)
            "write_file" | "edit_file" | "apply_patch" | "create_file" | "todowrite" => {
                // Check if path is within workspace
                if let Some(path) = self.extract_path(args) {
                    if self.is_within_workspace(&path) {
                        ActionTier::Write
                    } else {
                        ActionTier::Dangerous // Writing outside workspace
                    }
                } else {
                    ActionTier::Write
                }
            }

            // Unknown tools default to Write tier
            _ => ActionTier::Write,
        }
    }

    /// Evaluate an action and return a policy decision
    pub fn evaluate(&self, tool: &str, args: &serde_json::Value) -> PolicyDecision {
        let tier = self.classify_tier(tool, args);

        match tier {
            ActionTier::Safe => PolicyDecision::Allow,

            ActionTier::Write => {
                // Check for excluded paths
                if let Some(path) = self.extract_path(args) {
                    if self.is_excluded_path(&path) {
                        return PolicyDecision::Deny {
                            reason: format!("Access to sensitive file is denied: {}", path),
                        };
                    }
                    if !self.is_within_workspace(&path) {
                        return PolicyDecision::Deny {
                            reason: format!("Path is outside workspace: {}", path),
                        };
                    }
                }

                if self.config.auto_approve_writes {
                    PolicyDecision::Allow
                } else {
                    PolicyDecision::RequireApproval {
                        reason: format!("Write action requires approval: {}", tool),
                    }
                }
            }

            ActionTier::Dangerous => {
                if self.config.allow_dangerous {
                    PolicyDecision::RequireApproval {
                        reason: format!("Dangerous action requires explicit approval: {}", tool),
                    }
                } else {
                    PolicyDecision::Deny {
                        reason: format!(
                            "Dangerous action is blocked by policy: {}. Enable 'allow_dangerous_actions' to permit.",
                            tool
                        ),
                    }
                }
            }
        }
    }

    /// Extract file path from tool arguments
    fn extract_path(&self, args: &serde_json::Value) -> Option<String> {
        // Try common path argument names
        for key in &["path", "file", "file_path", "filepath", "target"] {
            if let Some(path) = args.get(key).and_then(|v| v.as_str()) {
                return Some(path.to_string());
            }
        }
        None
    }

    /// Check if a path is within the workspace
    pub fn is_within_workspace(&self, path: &str) -> bool {
        let path = Path::new(path);

        // Handle both absolute and relative paths
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.config.workspace_root.join(path)
        };

        // Canonicalize to resolve .. and symlinks
        match (
            abs_path.canonicalize(),
            self.config.workspace_root.canonicalize(),
        ) {
            (Ok(abs), Ok(root)) => abs.starts_with(&root),
            // If canonicalize fails, do a prefix check
            _ => {
                let path_str = abs_path.to_string_lossy().to_lowercase();
                let root_str = self.config.workspace_root.to_string_lossy().to_lowercase();
                path_str.starts_with(&root_str)
            }
        }
    }

    /// Check if a path matches excluded patterns
    fn is_excluded_path(&self, path: &str) -> bool {
        let path_lower = path.to_lowercase();

        for pattern in &self.config.excluded_paths {
            if pattern.starts_with("*.") {
                // Extension pattern
                let ext = &pattern[1..];
                if path_lower.ends_with(ext) {
                    return true;
                }
            } else if path_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }

        false
    }

    /// Redact secrets from text
    pub fn redact_secrets(&self, text: &str) -> String {
        let mut result = text.to_string();

        for pattern in &self.config.secret_patterns {
            result = pattern.replace_all(&result, "[REDACTED]").to_string();
        }

        result
    }

    /// Check if content contains secrets
    pub fn contains_secrets(&self, text: &str) -> bool {
        self.config.secret_patterns.iter().any(|p| p.is_match(text))
    }

    /// Filter file paths, removing excluded ones
    pub fn filter_paths(&self, paths: Vec<String>) -> Vec<String> {
        paths
            .into_iter()
            .filter(|p| !self.is_excluded_path(p))
            .collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_engine() -> PolicyEngine {
        PolicyEngine::new(PolicyConfig::new(PathBuf::from("/workspace")))
    }

    #[test]
    fn test_tier_classification() {
        let engine = test_engine();
        let empty_args = serde_json::json!({});

        assert_eq!(
            engine.classify_tier("read_file", &empty_args),
            ActionTier::Safe
        );
        assert_eq!(
            engine.classify_tier("search", &empty_args),
            ActionTier::Safe
        );
        assert_eq!(
            engine.classify_tier("write_file", &empty_args),
            ActionTier::Write
        );
        assert_eq!(
            engine.classify_tier("shell", &empty_args),
            ActionTier::Dangerous
        );
        assert_eq!(
            engine.classify_tier("git_push", &empty_args),
            ActionTier::Dangerous
        );
    }

    #[test]
    fn test_secret_redaction() {
        let engine = test_engine();

        let input = "API_KEY=test-key-1234567890abcdefghijklmnopqrstuvwxyz123456";
        let redacted = engine.redact_secrets(input);
        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("test-key-1234567890"));
    }

    #[test]
    fn test_excluded_paths() {
        let engine = test_engine();

        assert!(engine.is_excluded_path(".env"));
        assert!(engine.is_excluded_path("/path/to/.env.local"));
        assert!(engine.is_excluded_path("secrets.pem"));
        assert!(!engine.is_excluded_path("src/main.rs"));
    }
}
