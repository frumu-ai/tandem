fn value_str<'a>(obj: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| obj.get(*key).and_then(|v| v.as_str()))
}

fn value_bool(obj: &serde_json::Value, key: &str) -> Option<bool> {
    obj.get(key).and_then(|v| v.as_bool())
}

fn session_matches(value: &serde_json::Value, session_id: &str) -> bool {
    value_str(value, &["session_id", "sessionID", "sessionId"])
        .map(|v| v == session_id)
        .unwrap_or(false)
}

async fn requests_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let sid = active_session_id(msg, session_map).await;
    let client = reqwest::Client::new();

    let permissions = match add_auth(client.get(format!("{base_url}/permission")), api_token)
        .send()
        .await
    {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("requests").cloned())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let questions = match add_auth(client.get(format!("{base_url}/question")), api_token)
        .send()
        .await
    {
        Ok(resp) => resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let filtered_permissions: Vec<_> = if let Some(session_id) = sid.as_ref() {
        permissions
            .into_iter()
            .filter(|v| session_matches(v, session_id))
            .collect()
    } else {
        permissions
    };
    let filtered_questions: Vec<_> = if let Some(session_id) = sid.as_ref() {
        questions
            .into_iter()
            .filter(|v| session_matches(v, session_id))
            .collect()
    } else {
        questions
    };

    if filtered_permissions.is_empty() && filtered_questions.is_empty() {
        return "✅ No pending requests.".to_string();
    }

    let mut lines = Vec::new();
    for req in filtered_permissions.iter().take(8) {
        let id = value_str(req, &["id", "requestID", "request_id"]).unwrap_or("?");
        let tool = value_str(req, &["tool", "tool_name", "name"]).unwrap_or("tool");
        let approved = value_bool(req, "approved");
        let status = if approved == Some(true) {
            "approved"
        } else {
            "pending"
        };
        lines.push(format!(
            "🔐 `{}` {} ({})",
            &id[..8.min(id.len())],
            tool,
            status
        ));
    }
    for q in filtered_questions.iter().take(8) {
        let id = value_str(q, &["id", "questionID", "question_id"]).unwrap_or("?");
        let prompt = value_str(q, &["prompt", "question", "text"]).unwrap_or("question");
        lines.push(format!(
            "❓ `{}` {}",
            &id[..8.min(id.len())],
            prompt.chars().take(80).collect::<String>()
        ));
    }

    format!(
        "🧷 Pending requests ({} tool, {} question):\n{}",
        filtered_permissions.len(),
        filtered_questions.len(),
        lines.join("\n")
    )
}

async fn answer_question_text(
    question_id: String,
    answer: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "⚠️ No active session — cannot answer question.".to_string();
    };
    let client = reqwest::Client::new();
    let url = format!("{base_url}/sessions/{sid}/questions/{question_id}/answer");
    let resp = add_auth(client.post(url), api_token)
        .json(&serde_json::json!({ "answer": answer }))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {
            format!("✅ Answer submitted for question `{question_id}`.")
        }
        Ok(r) => format!("⚠️ Could not answer question (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Could not answer question: {e}"),
    }
}

async fn providers_text(base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch providers.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected providers response.".to_string();
    };
    let default = json
        .get("default")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let all = json
        .get("all")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if all.is_empty() {
        return "ℹ️ No providers available.".to_string();
    }
    let lines = all
        .iter()
        .take(20)
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let model_count = entry
                .get("models")
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0);
            format!("• {} ({} models)", id, model_count)
        })
        .collect::<Vec<_>>();
    format!("🧠 Providers (default: `{default}`):\n{}", lines.join("\n"))
}

async fn models_text(provider: Option<String>, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch models.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected models response.".to_string();
    };
    let all = json
        .get("all")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if all.is_empty() {
        return "ℹ️ No providers/models available.".to_string();
    }

    if let Some(provider_id) = provider {
        let target = all.iter().find(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.eq_ignore_ascii_case(&provider_id))
                .unwrap_or(false)
        });
        let Some(entry) = target else {
            return format!("⚠️ Provider `{provider_id}` not found. Use /providers.");
        };
        let models = entry
            .get("models")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        if models.is_empty() {
            return format!("ℹ️ Provider `{provider_id}` has no models listed.");
        }
        let mut model_ids = models.keys().cloned().collect::<Vec<_>>();
        model_ids.sort();
        let lines = model_ids
            .iter()
            .take(30)
            .map(|m| format!("• {m}"))
            .collect::<Vec<_>>();
        return format!("🧠 Models for `{provider_id}`:\n{}", lines.join("\n"));
    }

    let lines = all
        .iter()
        .map(|entry| {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let count = entry
                .get("models")
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0);
            format!("• {}: {} models", id, count)
        })
        .collect::<Vec<_>>();
    format!(
        "🧠 Model catalog by provider:\n{}\nUse `/models <provider>` to list model IDs.",
        lines.join("\n")
    )
}

async fn set_model_text(model_id: String, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/provider")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not fetch provider catalog.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected provider catalog response.".to_string();
    };

    let Some(default_provider) = json.get("default").and_then(|v| v.as_str()) else {
        return "⚠️ No default provider configured. Use desktop/TUI provider setup first."
            .to_string();
    };

    let provider_entry = json.get("all").and_then(|v| v.as_array()).and_then(|all| {
        all.iter().find(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id == default_provider)
                .unwrap_or(false)
        })
    });

    if let Some(entry) = provider_entry {
        let known = entry
            .get("models")
            .and_then(|v| v.as_object())
            .map(|models| models.contains_key(&model_id))
            .unwrap_or(true);
        if !known {
            return format!(
                "⚠️ Model `{}` not found for provider `{}`. Use `/models {}` first.",
                model_id, default_provider, default_provider
            );
        }
    }

    let mut provider_patch = serde_json::Map::new();
    provider_patch.insert(
        "default_model".to_string(),
        serde_json::json!(model_id.clone()),
    );
    let mut providers_patch = serde_json::Map::new();
    providers_patch.insert(
        default_provider.to_string(),
        serde_json::Value::Object(provider_patch),
    );
    let mut patch_map = serde_json::Map::new();
    patch_map.insert(
        "providers".to_string(),
        serde_json::Value::Object(providers_patch),
    );
    let patch = serde_json::Value::Object(patch_map);

    let resp = add_auth(client.patch(format!("{base_url}/config")), api_token)
        .json(&patch)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            format!(
                "✅ Model set to `{}` for default provider `{}`.",
                model_id, default_provider
            )
        }
        Ok(r) => format!("⚠️ Could not set model (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Could not set model: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

