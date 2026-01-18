// Tandem Stronghold Integration
// Secure encrypted storage for API keys and sensitive data

use crate::error::{Result, TandemError};
use serde::{Deserialize, Serialize};

/// Key identifiers for the stronghold vault
pub const STORE_NAME: &str = "api_keys";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApiKeyType {
    OpenRouter,
    OpenCodeZen,
    Anthropic,
    OpenAI,
    Custom(String),
}

impl ApiKeyType {
    pub fn to_key_name(&self) -> String {
        match self {
            ApiKeyType::OpenRouter => "openrouter_key".to_string(),
            ApiKeyType::OpenCodeZen => "opencode_zen_api_key".to_string(),
            ApiKeyType::Anthropic => "anthropic_key".to_string(),
            ApiKeyType::OpenAI => "openai_key".to_string(),
            ApiKeyType::Custom(name) => format!("custom_{}", name),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openrouter" => ApiKeyType::OpenRouter,
            "opencode_zen" | "opencodezen" => ApiKeyType::OpenCodeZen,
            "anthropic" => ApiKeyType::Anthropic,
            "openai" => ApiKeyType::OpenAI,
            other => ApiKeyType::Custom(other.to_string()),
        }
    }
}

/// Store an API key in the stronghold vault
/// Note: This is called from the Tauri command, which handles the actual stronghold operations
pub fn validate_api_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(TandemError::InvalidConfig(
            "API key cannot be empty".to_string(),
        ));
    }

    // Basic validation - keys should be reasonably long
    if key.len() < 10 {
        return Err(TandemError::InvalidConfig(
            "API key appears too short".to_string(),
        ));
    }

    Ok(())
}

/// Validate that a key type is supported
pub fn validate_key_type(key_type: &str) -> Result<ApiKeyType> {
    let api_key_type = ApiKeyType::from_str(key_type);

    // Custom keys need a valid name
    if let ApiKeyType::Custom(name) = &api_key_type {
        if name.is_empty() {
            return Err(TandemError::InvalidConfig(
                "Custom key type requires a name".to_string(),
            ));
        }
    }

    Ok(api_key_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_type_conversion() {
        assert!(matches!(
            ApiKeyType::from_str("openrouter"),
            ApiKeyType::OpenRouter
        ));
        assert!(matches!(
            ApiKeyType::from_str("opencode_zen"),
            ApiKeyType::OpenCodeZen
        ));
        assert!(matches!(
            ApiKeyType::from_str("opencodezen"),
            ApiKeyType::OpenCodeZen
        ));
        assert!(matches!(
            ApiKeyType::from_str("anthropic"),
            ApiKeyType::Anthropic
        ));
        assert!(matches!(ApiKeyType::from_str("openai"), ApiKeyType::OpenAI));

        if let ApiKeyType::Custom(name) = ApiKeyType::from_str("my_provider") {
            assert_eq!(name, "my_provider");
        } else {
            panic!("Expected Custom variant");
        }
    }

    #[test]
    fn test_validate_api_key() {
        assert!(validate_api_key("sk-1234567890abcdef").is_ok());
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("short").is_err());
    }
}
