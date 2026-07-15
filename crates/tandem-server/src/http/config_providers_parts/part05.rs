// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn ensure_provider_entry<'a>(
    wire: &'a mut WireProviderCatalog,
    provider_id: &str,
    provider_name: Option<&str>,
) -> &'a mut WireProviderEntry {
    if let Some(idx) = wire.all.iter().position(|entry| entry.id == provider_id) {
        return &mut wire.all[idx];
    }

    wire.all.push(WireProviderEntry {
        id: provider_id.to_string(),
        name: provider_name.map(|s| s.to_string()),
        models: HashMap::new(),
        catalog_source: None,
        catalog_status: None,
        catalog_message: None,
    });
    wire.all.last_mut().expect("provider entry just inserted")
}

fn canonical_provider_id(provider_id: &str) -> String {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        "llama.cpp" => "llama_cpp".to_string(),
        other => other.to_string(),
    }
}

fn provider_id_aliases(provider_id: &str) -> &'static [&'static str] {
    match canonical_provider_id(provider_id).as_str() {
        "llama_cpp" => &["llama_cpp", "llama.cpp"],
        _ => &[],
    }
}

fn known_provider_name(provider_id: &str) -> Option<&'static str> {
    match canonical_provider_id(provider_id).as_str() {
        "openrouter" => Some("OpenRouter"),
        "openai" => Some("OpenAI"),
        "openai-codex" => Some("OpenAI Codex"),
        "anthropic" => Some("Anthropic"),
        "ollama" => Some("Ollama"),
        "llama_cpp" => Some("llama.cpp"),
        "groq" => Some("Groq"),
        "mistral" => Some("Mistral"),
        "together" => Some("Together"),
        "cohere" => Some("Cohere"),
        "azure" => Some("Azure OpenAI-Compatible"),
        "bedrock" => Some("Bedrock-Compatible"),
        "vertex" => Some("Vertex-Compatible"),
        "copilot" => Some("GitHub Copilot-Compatible"),
        _ => None,
    }
}

fn ensure_known_provider_entries(wire: &mut WireProviderCatalog) {
    for provider_id in [
        "openrouter",
        "openai",
        "openai-codex",
        "anthropic",
        "ollama",
        "llama_cpp",
        "groq",
        "mistral",
        "together",
        "cohere",
        "azure",
        "bedrock",
        "vertex",
        "copilot",
    ] {
        ensure_provider_entry(wire, provider_id, known_provider_name(provider_id));
    }
}

fn merge_provider_model_map(
    wire: &mut WireProviderCatalog,
    provider_id: &str,
    provider_name: Option<&str>,
    models: HashMap<String, WireProviderModel>,
) {
    let entry = ensure_provider_entry(wire, provider_id, provider_name);
    for (model_id, model) in models {
        entry.models.insert(model_id, model);
    }
}

fn config_provider_root<'a>(cfg: &'a Value) -> Option<&'a serde_json::Map<String, Value>> {
    cfg.get("providers")
        .and_then(Value::as_object)
        .or_else(|| cfg.get("provider").and_then(Value::as_object))
}

fn provider_config_value<'a>(cfg: &'a Value, provider_id: &str) -> Option<&'a Value> {
    let root = config_provider_root(cfg)?;
    root.get(provider_id).or_else(|| {
        provider_id_aliases(provider_id)
            .iter()
            .find_map(|alias| root.get(*alias))
    })
}

fn merge_provider_models_from_config(wire: &mut WireProviderCatalog, cfg: &Value) -> Vec<String> {
    let Some(provider_root) = config_provider_root(cfg) else {
        return Vec::new();
    };

    let mut merged = Vec::new();
    for (provider_id, provider_value) in provider_root {
        let provider_id = canonical_provider_id(provider_id);
        let provider_name = provider_value
            .get("name")
            .and_then(|v| v.as_str())
            .or(known_provider_name(&provider_id))
            .or(Some(provider_id.as_str()));

        let mut model_map: HashMap<String, WireProviderModel> = HashMap::new();
        if let Some(models_obj) = provider_value.get("models").and_then(|v| v.as_object()) {
            for (model_id, model_value) in models_obj {
                let display_name = model_value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| Some(model_id.to_string()));
                let context = model_value
                    .get("limit")
                    .and_then(|v| v.get("context"))
                    .and_then(|v| v.as_u64())
                    .or_else(|| model_value.get("context_length").and_then(|v| v.as_u64()))
                    .map(|v| v as u32);

                model_map.insert(
                    model_id.to_string(),
                    WireProviderModel {
                        name: display_name,
                        limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
                    },
                );
            }
        }

        if !model_map.is_empty() {
            merge_provider_model_map(wire, &provider_id, provider_name, model_map);
            merged.push(provider_id);
        }
    }

    merged
}

fn provider_default_url(provider_id: &str) -> Option<&'static str> {
    match canonical_provider_id(provider_id).as_str() {
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "openai" => Some("https://api.openai.com/v1"),
        "openai-codex" => Some(OPENAI_CODEX_API_BASE_URL),
        "llama_cpp" => Some("http://127.0.0.1:8080/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "together" => Some("https://api.together.xyz/v1"),
        _ => None,
    }
}

fn codex_starter_models() -> HashMap<String, WireProviderModel> {
    openai_codex_supported_model_rows()
        .iter()
        .map(|(id, name)| {
            (
                id.to_string(),
                WireProviderModel {
                    name: Some(name.to_string()),
                    limit: Some(WireProviderModelLimit {
                        context: Some(272_000),
                    }),
                },
            )
        })
        .collect()
}

fn provider_env_candidates(provider_id: &str) -> Vec<String> {
    let normalized = provider_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .to_ascii_uppercase();

    let mut out = vec![format!("{}_API_KEY", normalized)];
    match provider_id.to_ascii_lowercase().as_str() {
        "openai" => out.push("OPENAI_API_KEY".to_string()),
        "openrouter" => out.push("OPENROUTER_API_KEY".to_string()),
        "anthropic" => out.push("ANTHROPIC_API_KEY".to_string()),
        "groq" => out.push("GROQ_API_KEY".to_string()),
        "mistral" => out.push("MISTRAL_API_KEY".to_string()),
        "together" => out.push("TOGETHER_API_KEY".to_string()),
        "azure" => out.push("AZURE_OPENAI_API_KEY".to_string()),
        "vertex" => out.push("VERTEX_API_KEY".to_string()),
        "bedrock" => out.push("BEDROCK_API_KEY".to_string()),
        "copilot" => out.push("GITHUB_TOKEN".to_string()),
        "cohere" => out.push("COHERE_API_KEY".to_string()),
        _ => {}
    }
    out.sort();
    out.dedup();
    out
}

fn provider_has_env_secret(provider_id: &str) -> bool {
    provider_env_candidates(provider_id).into_iter().any(|key| {
        std::env::var(&key)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

fn hosted_managed_from_config(cfg: &Value) -> bool {
    cfg.get("hosted")
        .and_then(Value::as_object)
        .and_then(|hosted| hosted.get("managed"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn hosted_public_url_from_config(cfg: &Value) -> Option<String> {
    cfg.get("hosted")
        .and_then(Value::as_object)
        .and_then(|hosted| hosted.get("public_url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn provider_requires_api_key(provider_id: &str) -> bool {
    !matches!(
        canonical_provider_id(provider_id).as_str(),
        "local" | "ollama" | "llama_cpp"
    )
}

fn provider_oauth_redirect_uri_for_base(base_url: &str, provider_id: &str) -> String {
    let base = base_url.trim().trim_end_matches('/').to_string();
    if canonical_provider_id(provider_id) == OPENAI_CODEX_PROVIDER_ID {
        return format!("{base}/provider/{provider_id}/oauth/callback");
    }
    format!("{base}/provider/{provider_id}/oauth/callback")
}

fn provider_oauth_redirect_uri_for_control_panel_base(base_url: &str, provider_id: &str) -> String {
    let base = base_url.trim().trim_end_matches('/').to_string();
    format!("{base}/api/engine/provider/{provider_id}/oauth/callback")
}

fn generate_oauth_state() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        Uuid::new_v4(),
        Uuid::new_v4()
    ))
}

fn generate_pkce_pair() -> (String, String) {
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        Uuid::new_v4(),
        Uuid::new_v4()
    ));
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

fn build_openai_codex_authorization_url(
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    let query = vec![
        ("response_type", "code".to_string()),
        ("client_id", OPENAI_CODEX_OAUTH_CLIENT_ID.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("scope", "openid profile email offline_access".to_string()),
        ("code_challenge", code_challenge.to_string()),
        ("code_challenge_method", "S256".to_string()),
        ("id_token_add_organizations", "true".to_string()),
        ("codex_cli_simplified_flow", "true".to_string()),
        ("state", state.to_string()),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={}", urlencoding::encode(&value)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{OPENAI_CODEX_OAUTH_ISSUER}/oauth/authorize?{query}")
}

fn openai_codex_local_callback_shutdown_slot() -> &'static Mutex<Option<oneshot::Sender<()>>> {
    static SLOT: OnceLock<Mutex<Option<oneshot::Sender<()>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}
