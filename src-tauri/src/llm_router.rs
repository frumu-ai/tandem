// Tandem LLM Router
// Provider-agnostic routing to different LLM backends

use crate::error::{Result, TandemError};
use crate::state::ProvidersConfig;
use serde::{Deserialize, Serialize};

/// Supported LLM providers
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    OpenRouter,
    Anthropic,
    OpenAI,
    Ollama,
    Custom,
}

impl Provider {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openrouter" => Some(Provider::OpenRouter),
            "anthropic" => Some(Provider::Anthropic),
            "openai" => Some(Provider::OpenAI),
            "ollama" => Some(Provider::Ollama),
            "custom" => Some(Provider::Custom),
            _ => None,
        }
    }
}

/// Chat message format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Request to send to an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// Response from an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMResponse {
    pub content: String,
    pub model: String,
    pub provider: String,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Streaming chunk from an LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub delta: String,
    pub finish_reason: Option<String>,
}

/// LLM Router handles routing requests to the appropriate provider
pub struct LLMRouter {
    config: ProvidersConfig,
}

impl LLMRouter {
    pub fn new(config: ProvidersConfig) -> Self {
        Self { config }
    }

    /// Update the configuration
    pub fn update_config(&mut self, config: ProvidersConfig) {
        self.config = config;
    }

    /// Get the default provider
    pub fn get_default_provider(&self) -> Option<Provider> {
        if self.config.openrouter.default && self.config.openrouter.enabled {
            return Some(Provider::OpenRouter);
        }
        if self.config.anthropic.default && self.config.anthropic.enabled {
            return Some(Provider::Anthropic);
        }
        if self.config.openai.default && self.config.openai.enabled {
            return Some(Provider::OpenAI);
        }
        if self.config.ollama.default && self.config.ollama.enabled {
            return Some(Provider::Ollama);
        }

        // Fallback to first enabled provider
        if self.config.openrouter.enabled {
            return Some(Provider::OpenRouter);
        }
        if self.config.anthropic.enabled {
            return Some(Provider::Anthropic);
        }
        if self.config.openai.enabled {
            return Some(Provider::OpenAI);
        }
        if self.config.ollama.enabled {
            return Some(Provider::Ollama);
        }

        None
    }

    /// Get the endpoint for a provider
    pub fn get_endpoint(&self, provider: Provider) -> Option<String> {
        match provider {
            Provider::OpenRouter => {
                if self.config.openrouter.enabled {
                    Some(self.config.openrouter.endpoint.clone())
                } else {
                    None
                }
            }
            Provider::Anthropic => {
                if self.config.anthropic.enabled {
                    Some(self.config.anthropic.endpoint.clone())
                } else {
                    None
                }
            }
            Provider::OpenAI => {
                if self.config.openai.enabled {
                    Some(self.config.openai.endpoint.clone())
                } else {
                    None
                }
            }
            Provider::Ollama => {
                if self.config.ollama.enabled {
                    Some(self.config.ollama.endpoint.clone())
                } else {
                    None
                }
            }
            Provider::Custom => {
                // Return first enabled custom provider
                self.config.custom.first().map(|c| c.endpoint.clone())
            }
        }
    }

    /// Get the default model for a provider
    pub fn get_default_model(&self, provider: Provider) -> Option<String> {
        match provider {
            Provider::OpenRouter => self.config.openrouter.model.clone(),
            Provider::Anthropic => self.config.anthropic.model.clone(),
            Provider::OpenAI => self.config.openai.model.clone(),
            Provider::Ollama => self.config.ollama.model.clone(),
            Provider::Custom => self.config.custom.first().and_then(|c| c.model.clone()),
        }
    }

    /// Format a request for a specific provider
    pub fn format_request(
        &self,
        provider: Provider,
        request: &LLMRequest,
    ) -> Result<serde_json::Value> {
        match provider {
            Provider::OpenRouter | Provider::OpenAI => {
                // OpenAI-compatible format
                Ok(serde_json::json!({
                    "model": request.model.clone().unwrap_or_else(||
                        self.get_default_model(provider).unwrap_or_else(|| "gpt-4".to_string())
                    ),
                    "messages": request.messages,
                    "max_tokens": request.max_tokens.unwrap_or(4096),
                    "temperature": request.temperature.unwrap_or(0.7),
                    "stream": request.stream.unwrap_or(true),
                }))
            }
            Provider::Anthropic => {
                // Anthropic format
                let system_message = request
                    .messages
                    .iter()
                    .find(|m| m.role == "system")
                    .map(|m| m.content.clone());

                let messages: Vec<_> = request
                    .messages
                    .iter()
                    .filter(|m| m.role != "system")
                    .map(|m| {
                        serde_json::json!({
                            "role": m.role,
                            "content": m.content,
                        })
                    })
                    .collect();

                let mut body = serde_json::json!({
                    "model": request.model.clone().unwrap_or_else(||
                        self.get_default_model(provider).unwrap_or_else(|| "claude-sonnet-4-20250514".to_string())
                    ),
                    "messages": messages,
                    "max_tokens": request.max_tokens.unwrap_or(4096),
                    "stream": request.stream.unwrap_or(true),
                });

                if let Some(system) = system_message {
                    body["system"] = serde_json::Value::String(system);
                }

                Ok(body)
            }
            Provider::Ollama => {
                // Ollama format
                Ok(serde_json::json!({
                    "model": request.model.clone().unwrap_or_else(||
                        self.get_default_model(provider).unwrap_or_else(|| "llama3.2".to_string())
                    ),
                    "messages": request.messages,
                    "stream": request.stream.unwrap_or(true),
                    "options": {
                        "temperature": request.temperature.unwrap_or(0.7),
                    }
                }))
            }
            Provider::Custom => {
                // Default to OpenAI-compatible format for custom providers
                Ok(serde_json::json!({
                    "model": request.model.clone().unwrap_or_else(|| "default".to_string()),
                    "messages": request.messages,
                    "max_tokens": request.max_tokens.unwrap_or(4096),
                    "temperature": request.temperature.unwrap_or(0.7),
                    "stream": request.stream.unwrap_or(true),
                }))
            }
        }
    }

    /// Get the chat completions endpoint path for a provider
    pub fn get_completions_path(&self, provider: Provider) -> &'static str {
        match provider {
            Provider::OpenRouter => "/api/v1/chat/completions",
            Provider::Anthropic => "/v1/messages",
            Provider::OpenAI => "/v1/chat/completions",
            Provider::Ollama => "/api/chat",
            Provider::Custom => "/v1/chat/completions", // Default to OpenAI-compatible
        }
    }

    /// Get required headers for a provider
    pub fn get_headers(&self, provider: Provider, api_key: &str) -> Vec<(String, String)> {
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];

        match provider {
            Provider::OpenRouter => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", api_key)));
                headers.push(("HTTP-Referer".to_string(), "https://tandem.app".to_string()));
                headers.push(("X-Title".to_string(), "Tandem".to_string()));
            }
            Provider::Anthropic => {
                headers.push(("x-api-key".to_string(), api_key.to_string()));
                headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
            }
            Provider::OpenAI => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", api_key)));
            }
            Provider::Ollama => {
                // Ollama typically doesn't need auth for local usage
            }
            Provider::Custom => {
                headers.push(("Authorization".to_string(), format!("Bearer {}", api_key)));
            }
        }

        headers
    }

    /// Parse a response from a provider
    pub fn parse_response(
        &self,
        provider: Provider,
        response: &serde_json::Value,
    ) -> Result<LLMResponse> {
        match provider {
            Provider::OpenRouter | Provider::OpenAI => {
                let content = response["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let model = response["model"].as_str().unwrap_or("unknown").to_string();

                let usage = response.get("usage").map(|u| TokenUsage {
                    prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                    completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                    total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
                });

                let finish_reason = response["choices"][0]["finish_reason"]
                    .as_str()
                    .map(|s| s.to_string());

                Ok(LLMResponse {
                    content,
                    model,
                    provider: format!("{:?}", provider),
                    usage,
                    finish_reason,
                })
            }
            Provider::Anthropic => {
                let content = response["content"][0]["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let model = response["model"].as_str().unwrap_or("unknown").to_string();

                let usage = response.get("usage").map(|u| TokenUsage {
                    prompt_tokens: u["input_tokens"].as_u64().unwrap_or(0) as u32,
                    completion_tokens: u["output_tokens"].as_u64().unwrap_or(0) as u32,
                    total_tokens: (u["input_tokens"].as_u64().unwrap_or(0)
                        + u["output_tokens"].as_u64().unwrap_or(0))
                        as u32,
                });

                let finish_reason = response["stop_reason"].as_str().map(|s| s.to_string());

                Ok(LLMResponse {
                    content,
                    model,
                    provider: "Anthropic".to_string(),
                    usage,
                    finish_reason,
                })
            }
            Provider::Ollama => {
                let content = response["message"]["content"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                let model = response["model"].as_str().unwrap_or("unknown").to_string();

                Ok(LLMResponse {
                    content,
                    model,
                    provider: "Ollama".to_string(),
                    usage: None,
                    finish_reason: Some("stop".to_string()),
                })
            }
            Provider::Custom => {
                // Try OpenAI format first
                self.parse_response(Provider::OpenAI, response)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ProviderConfig;

    fn create_test_config() -> ProvidersConfig {
        ProvidersConfig {
            openrouter: ProviderConfig {
                enabled: true,
                default: true,
                endpoint: "https://openrouter.ai/api/v1".to_string(),
                model: Some("anthropic/claude-sonnet-4".to_string()),
            },
            anthropic: ProviderConfig {
                enabled: false,
                default: false,
                endpoint: "https://api.anthropic.com".to_string(),
                model: None,
            },
            openai: ProviderConfig {
                enabled: false,
                default: false,
                endpoint: "https://api.openai.com/v1".to_string(),
                model: None,
            },
            ollama: ProviderConfig {
                enabled: true,
                default: false,
                endpoint: "http://localhost:11434".to_string(),
                model: Some("llama3.2".to_string()),
            },
            custom: vec![],
        }
    }

    #[test]
    fn test_get_default_provider() {
        let config = create_test_config();
        let router = LLMRouter::new(config);

        assert_eq!(router.get_default_provider(), Some(Provider::OpenRouter));
    }

    #[test]
    fn test_get_endpoint() {
        let config = create_test_config();
        let router = LLMRouter::new(config);

        assert_eq!(
            router.get_endpoint(Provider::OpenRouter),
            Some("https://openrouter.ai/api/v1".to_string())
        );
        assert_eq!(
            router.get_endpoint(Provider::Ollama),
            Some("http://localhost:11434".to_string())
        );
        assert_eq!(router.get_endpoint(Provider::Anthropic), None); // Disabled
    }

    #[test]
    fn test_format_request() {
        let config = create_test_config();
        let router = LLMRouter::new(config);

        let request = LLMRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            model: None,
            max_tokens: Some(1000),
            temperature: Some(0.5),
            stream: Some(true),
        };

        let formatted = router
            .format_request(Provider::OpenRouter, &request)
            .unwrap();
        assert!(formatted["messages"].is_array());
        assert_eq!(formatted["max_tokens"], 1000);
        assert_eq!(formatted["temperature"], 0.5);
    }
}
