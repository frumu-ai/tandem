fn remote_catalog_support_message(provider_id: &str) -> String {
    match provider_id {
        "openai-codex" => {
            "Codex account models use a Tandem starter catalog. Live discovery is not enabled here."
                .to_string()
        }
        "azure" | "vertex" | "bedrock" | "copilot" | "ollama" => {
            "Live model discovery is unavailable for this provider. Enter a model ID manually."
                .to_string()
        }
        "anthropic" | "cohere" => {
            "This provider does not currently expose a reliable live model catalog here. Enter a model ID manually."
                .to_string()
        }
        _ => "Live model discovery is unavailable. Enter a model ID manually.".to_string(),
    }
}

fn provider_config_api_key(
    cfg: &Value,
    provider_id: &str,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Option<String> {
    allow_shared_auth_sources
        .then(|| {
            provider_config_value(cfg, provider_id)
                .and_then(|entry| entry.get("api_key").or_else(|| entry.get("apiKey")))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "x")
                .map(str::to_string)
        })
        .flatten()
        .or_else(|| {
            runtime_auth
                .get(&provider_id.to_ascii_lowercase())
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            persisted_auth
                .get(&provider_id.to_ascii_lowercase())
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            allow_shared_auth_sources
                .then(|| {
                    provider_env_candidates(provider_id)
                        .into_iter()
                        .find_map(|key| std::env::var(&key).ok())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                })
                .flatten()
        })
}

#[cfg(test)]
mod provider_auth_resolution_tests {
    use super::*;

    #[test]
    fn provider_api_key_resolution_preserves_runtime_precedence_over_env_fallback() {
        let provider_id = format!("review-precedence-{}", uuid::Uuid::new_v4());
        let env_key = provider_env_candidates(&provider_id)
            .into_iter()
            .next()
            .expect("provider env key");
        std::env::set_var(&env_key, "env-key");

        let runtime_auth = HashMap::from([(provider_id.to_ascii_lowercase(), "runtime-key".to_string())]);
        let persisted_auth =
            HashMap::from([(provider_id.to_ascii_lowercase(), "persisted-key".to_string())]);
        assert_eq!(
            provider_config_api_key(&json!({}), &provider_id, &runtime_auth, &persisted_auth, true)
                .as_deref(),
            Some("runtime-key")
        );
        assert_eq!(
            provider_config_api_key(&json!({}), &provider_id, &HashMap::new(), &persisted_auth, true)
                .as_deref(),
            Some("persisted-key")
        );
        assert!(
            provider_config_api_key(
                &json!({}),
                &provider_id,
                &HashMap::new(),
                &HashMap::new(),
                false,
            )
            .is_none(),
            "explicit tenants must not use shared env fallback keys"
        );

        std::env::remove_var(env_key);
    }
}

/// Validate provider base URL to prevent SSRF attacks.
/// Rejects private IP addresses and non-HTTPS schemes.
fn validate_provider_url(url_str: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url_str).map_err(|_| "invalid provider URL format")?;
    let scheme = parsed.scheme();
    if !matches!(scheme, "https" | "http") {
        return Err("provider URLs must use http or https".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "invalid provider URL format".to_string())?
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    let is_local_dev_host = host == "localhost" || host == "::1" || host == "127.0.0.1";
    if scheme == "http" && !(cfg!(any(test, debug_assertions)) && is_local_dev_host) {
        return Err("provider URLs must use HTTPS for non-localhost addresses".to_string());
    }
    if cfg!(any(test, debug_assertions)) && is_local_dev_host {
        return Ok(());
    }
    if is_local_dev_host
        || host.ends_with(".local")
        || host.ends_with(".localdomain")
        || host.ends_with(".internal")
    {
        return Err("provider URL cannot use localhost or private hostnames".to_string());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        let blocked = match ip {
            IpAddr::V4(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || ip.is_private()
                    || ip.is_link_local()
            }
            IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
            }
        };
        if blocked {
            return Err("provider URL cannot use private or reserved IP addresses".to_string());
        }
    }

    Ok(())
}

fn provider_base_url(cfg: &Value, provider_id: &str) -> Option<String> {
    provider_config_value(cfg, provider_id)
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| provider_default_url(provider_id).map(str::to_string))
}

fn normalize_openai_catalog_base(input: &str) -> String {
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/chat/completions", "/completions", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    while let Some(prefix) = base.strip_suffix("/v1") {
        if prefix.ends_with("/v1") {
            base = prefix.to_string();
            continue;
        }
        break;
    }
    if base.ends_with("/v1") {
        base
    } else {
        format!("{}/v1", base.trim_end_matches('/'))
    }
}

fn parse_openrouter_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        let context = item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                item.get("top_provider")
                    .and_then(|v| v.get("context_length"))
                    .and_then(|v| v.as_u64())
            })
            .map(|v| v as u32);

        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name,
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

fn parse_openai_compatible_model_payload(
    body: &Value,
) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        let context = item
            .get("context_window")
            .and_then(|v| v.as_u64())
            .or_else(|| item.get("context_length").and_then(|v| v.as_u64()))
            .map(|v| v as u32);
        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name,
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_openrouter_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let api_key = provider_config_api_key(
        cfg,
        "openrouter",
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    )
        .or_else(|| {
            allow_shared_auth_sources
                .then(|| std::env::var("OPENCODE_OPENROUTER_API_KEY").ok())
                .flatten()
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(api_key) = api_key else {
        return Err(
            "OpenRouter requires an API key before live model discovery is available.".to_string(),
        );
    };

    let client = reqwest::Client::new();
    let mut req = client
        .get("https://openrouter.ai/api/v1/models")
        .timeout(Duration::from_secs(20));
    req = req.bearer_auth(api_key);
    let resp = req
        .send()
        .await
        .map_err(|err| format!("Failed to fetch OpenRouter models: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "OpenRouter model catalog request failed with status {}",
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode OpenRouter models: {err}"))?;
    parse_openrouter_model_payload(&body)
        .ok_or_else(|| "OpenRouter returned an empty or invalid model catalog.".to_string())
}

async fn fetch_openai_compatible_models(
    provider_id: &str,
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let api_key = provider_config_api_key(
        cfg,
        provider_id,
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    );
    if api_key.is_none() && !provider_supports_optional_auth(provider_id) {
        return Err(format!(
            "{} requires an API key before live model discovery is available.",
            known_provider_name(provider_id).unwrap_or(provider_id)
        ));
    }
    let Some(base_url) = provider_base_url(cfg, provider_id) else {
        return Err("No provider base URL is configured for live model discovery.".to_string());
    };

    // SECURITY: Validate provider URL to prevent SSRF attacks
    validate_provider_url(&base_url)?;

    let url = format!("{}/models", normalize_openai_catalog_base(&base_url));
    let client = reqwest::Client::new();
    let request = client.get(&url).timeout(Duration::from_secs(20));
    let request = if let Some(api_key) = api_key {
        request.bearer_auth(api_key)
    } else {
        request
    };
    let resp = request
        .send()
        .await
        .map_err(|err| format!("Failed to fetch model catalog from {provider_id}: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "{} model catalog request failed with status {}",
            provider_id,
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode model catalog from {provider_id}: {err}"))?;
    parse_openai_compatible_model_payload(&body).ok_or_else(|| {
        format!("{provider_id} returned an empty or invalid OpenAI-compatible model catalog.")
    })
}

async fn fetch_openai_codex_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(
        cfg,
        OPENAI_CODEX_PROVIDER_ID,
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    )
    else {
        return Err(
            "OpenAI Codex requires a connected account before live model discovery is available."
                .to_string(),
        );
    };
    let base_url = provider_base_url(cfg, OPENAI_CODEX_PROVIDER_ID)
        .unwrap_or_else(|| OPENAI_CODEX_API_BASE_URL.to_string());
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|err| format!("Failed to fetch OpenAI Codex models: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "OpenAI Codex model catalog request failed with status {}",
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode OpenAI Codex models: {err}"))?;
    parse_openai_compatible_model_payload(&body)
        .ok_or_else(|| "OpenAI Codex returned an empty or invalid model catalog.".to_string())
}

fn normalize_cohere_catalog_base(input: &str) -> String {
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/v1", "/v2", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    format!("{}/v1", base.trim_end_matches('/'))
}

fn parse_anthropic_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        out.insert(
            model_id.to_string(),
            WireProviderModel { name, limit: None },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_anthropic_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(
        cfg,
        "anthropic",
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    )
    else {
        return Err(
            "Anthropic requires an API key before live model discovery is available.".to_string(),
        );
    };
    let resp = reqwest::Client::new()
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|err| format!("Failed to fetch Anthropic models: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Anthropic model catalog request failed with status {}",
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode Anthropic models: {err}"))?;
    parse_anthropic_model_payload(&body)
        .ok_or_else(|| "Anthropic returned an empty or invalid model catalog.".to_string())
}

fn parse_cohere_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body
        .get("models")
        .and_then(|v| v.as_array())
        .or_else(|| body.get("data").and_then(|v| v.as_array()))?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("id").and_then(|v| v.as_str()))
        else {
            continue;
        };
        let context = item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name: Some(model_id.to_string()),
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_cohere_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(
        cfg,
        "cohere",
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    ) else {
        return Err(
            "Cohere requires an API key before live model discovery is available.".to_string(),
        );
    };
    let base_url = provider_config_value(cfg, "cohere")
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("https://api.cohere.com/v2");
    let url = format!("{}/models", normalize_cohere_catalog_base(base_url));
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|err| format!("Failed to fetch Cohere models: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Cohere model catalog request failed with status {}",
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode Cohere models: {err}"))?;
    parse_cohere_model_payload(&body)
        .ok_or_else(|| "Cohere returned an empty or invalid model catalog.".to_string())
}

async fn fetch_remote_provider_models(
    provider_id: &str,
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> ProviderCatalogFetchResult {
    match provider_id {
        "openai-codex" => {
            match fetch_openai_codex_models(
                cfg,
                runtime_auth,
                persisted_auth,
                allow_shared_auth_sources,
            )
            .await
            {
                Ok(models) => ProviderCatalogFetchResult::Remote { models },
                Err(message) => {
                    tracing::debug!(
                        "openai-codex catalog discovery fell back to static: {message}"
                    );
                    ProviderCatalogFetchResult::Static {
                        models: codex_starter_models(),
                    }
                }
            }
        }
        "openrouter" => match fetch_openrouter_models(
            cfg,
            runtime_auth,
            persisted_auth,
            allow_shared_auth_sources,
        )
        .await
        {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::debug!("openrouter catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "openai" | "llama_cpp" | "groq" | "mistral" | "together" => {
            match fetch_openai_compatible_models(
                provider_id,
                cfg,
                runtime_auth,
                persisted_auth,
                allow_shared_auth_sources,
            )
            .await
            {
                Ok(models) => ProviderCatalogFetchResult::Remote { models },
                Err(message) => {
                    if message.contains("requires an API key") {
                        ProviderCatalogFetchResult::Unavailable { message }
                    } else {
                        tracing::warn!("{provider_id} catalog discovery failed: {message}");
                        ProviderCatalogFetchResult::Error { message }
                    }
                }
            }
        }
        "anthropic" => match fetch_anthropic_models(
            cfg,
            runtime_auth,
            persisted_auth,
            allow_shared_auth_sources,
        )
        .await
        {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::warn!("anthropic catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "cohere" => match fetch_cohere_models(
            cfg,
            runtime_auth,
            persisted_auth,
            allow_shared_auth_sources,
        )
        .await
        {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::warn!("cohere catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "azure" | "vertex" | "bedrock" | "copilot" | "ollama" => {
            ProviderCatalogFetchResult::Unavailable {
                message: remote_catalog_support_message(provider_id),
            }
        }
        _ => ProviderCatalogFetchResult::Unavailable {
            message: "Live model discovery is unavailable. Enter a model ID manually.".to_string(),
        },
    }
}

fn provider_supports_optional_auth(provider_id: &str) -> bool {
    matches!(canonical_provider_id(provider_id).as_str(), "llama_cpp")
}
