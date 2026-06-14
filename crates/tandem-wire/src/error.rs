use serde::{Deserialize, Serialize};

/// Stable machine-readable error codes returned by Tandem HTTP APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// The request did not include a valid engine API token.
    AuthRequired,
    /// The request tenant context could not be verified or authorized.
    TenantContextDenied,
    /// The requested resource belongs to another tenant.
    TenantScopeDenied,
    /// The request body or parameter values failed validation.
    ValidationFailed,
    /// A requested session does not exist or is not visible to the caller.
    SessionNotFound,
    /// A session already has an active run.
    SessionRunConflict,
    /// The request was throttled by a rate limiter.
    RateLimited,
    /// The engine accepted the request but did not complete before the API timeout.
    PromptTimeout,
    /// The engine has not finished startup yet.
    EngineStarting,
    /// The engine failed startup and cannot serve the request.
    EngineStartupFailed,
    /// A human approval or permission reply was malformed.
    ApprovalReplyInvalid,
    /// The requested approval or permission item was not found.
    ApprovalRequestNotFound,
    /// Persisting or loading approval state failed.
    ApprovalPersistenceFailed,
    /// An MCP HTTP registration or OAuth request was denied.
    McpRequestDenied,
    /// Stdio MCP transports may not be registered through the HTTP API.
    McpStdioTransportDenied,
    /// An MCP refresh or reconnect request failed.
    McpRefreshFailed,
    /// An MCP OAuth flow failed.
    McpOauthFailed,
    /// A skill or memory API request failed.
    SkillsError,
    /// An optimization API request failed validation.
    OptimizationValidationFailed,
    /// The requested optimization resource was not found.
    OptimizationNotFound,
    /// The optimization action conflicts with the current state.
    OptimizationConflict,
    /// A storage or persistence operation failed.
    PersistenceFailed,
    /// An internal server error occurred.
    InternalError,
}

impl ErrorCode {
    /// Whether retrying the exact request later may succeed without changing input.
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited
                | Self::PromptTimeout
                | Self::EngineStarting
                | Self::McpRefreshFailed
                | Self::PersistenceFailed
                | Self::ApprovalPersistenceFailed
                | Self::InternalError
        )
    }
}

/// Standard HTTP error envelope returned by Tandem APIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorEnvelope {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
    pub retryable: bool,
}

impl ErrorEnvelope {
    pub fn new(error: impl Into<String>, code: ErrorCode) -> Self {
        Self {
            error: error.into(),
            code: Some(code),
            retryable: code.retryable(),
        }
    }

    pub fn with_retryable(error: impl Into<String>, code: ErrorCode, retryable: bool) -> Self {
        Self {
            error: error.into(),
            code: Some(code),
            retryable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_serializes_as_screaming_snake_case() {
        let value = serde_json::to_value(ErrorCode::TenantContextDenied).unwrap();
        assert_eq!(value, serde_json::json!("TENANT_CONTEXT_DENIED"));
    }

    #[test]
    fn error_envelope_includes_retryable_hint() {
        let envelope = ErrorEnvelope::new("engine still starting", ErrorCode::EngineStarting);
        let value = serde_json::to_value(envelope).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "error": "engine still starting",
                "code": "ENGINE_STARTING",
                "retryable": true
            })
        );
    }
}
