use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    #[serde(alias = "providerID", alias = "providerId")]
    pub provider_id: String,
    #[serde(alias = "modelID", alias = "modelId")]
    pub model_id: String,
}

/// Generic sampling parameters callers can set per-session (default) or
/// per-prompt (override).
///
/// All fields are optional. Omitting a field preserves the engine's existing
/// behavior — no value is sent to the provider for that field. Per-provider
/// mapping and range clamping happens at the provider-adapter boundary, not
/// here, so this type stays a plain transport-level contract.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct SamplingParams {
    /// Sampling temperature. Generic range `[0.0, 2.0]`; clamped per provider
    /// (e.g. Anthropic caps at `1.0`). Lower values produce more deterministic
    /// output — useful for strict-JSON roles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Nucleus sampling probability mass. Range `[0.0, 1.0]`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "topP",
        alias = "top_p"
    )]
    pub top_p: Option<f32>,
    /// Maximum tokens to generate. When set, overrides the engine's default
    /// max-tokens budget for the request.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "maxTokens",
        alias = "max_tokens"
    )]
    pub max_tokens: Option<u32>,
}

impl SamplingParams {
    /// Returns `true` when no sampling parameter is set.
    pub fn is_empty(&self) -> bool {
        self.temperature.is_none() && self.top_p.is_none() && self.max_tokens.is_none()
    }

    /// Resolve `self` (override, e.g. per-prompt) over `base` (default, e.g.
    /// session-level). Each field is taken from `self` when present, otherwise
    /// from `base`. This is the documented precedence: per-prompt wins over the
    /// session default, field by field.
    pub fn resolve_over(self, base: SamplingParams) -> SamplingParams {
        SamplingParams {
            temperature: self.temperature.or(base.temperature),
            top_p: self.top_p.or(base.top_p),
            max_tokens: self.max_tokens.or(base.max_tokens),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider_id: String,
    pub display_name: String,
    pub context_window: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
}
