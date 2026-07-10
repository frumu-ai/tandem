fn provider_oauth_public_base_url_from_env() -> Option<String> {
    [
        "TANDEM_CONTROL_PANEL_PUBLIC_URL",
        "HOSTED_CONTROL_PANEL_PUBLIC_URL",
        "HOSTED_PUBLIC_URL",
    ]
    .into_iter()
    .find_map(|key| {
        std::env::var(key)
            .ok()
            .and_then(|value| normalize_provider_oauth_base_url(&value))
    })
}

fn provider_oauth_public_base_url_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    for name in ["origin", "referer"] {
        if let Some(base) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(normalize_provider_oauth_base_url)
        {
            return Some(base);
        }
    }

    let host = headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or(value).trim())
        .filter(|value| !value.is_empty())?;
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or(value).trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("https");
    normalize_provider_oauth_base_url(&format!("{proto}://{host}"))
}

fn provider_oauth_public_panel_base_url(
    cfg: &Value,
    headers: &axum::http::HeaderMap,
) -> Option<String> {
    hosted_public_url_from_config(cfg)
        .and_then(|value| normalize_provider_oauth_base_url(&value))
        .or_else(provider_oauth_public_base_url_from_env)
        .or_else(|| provider_oauth_public_base_url_from_headers(headers))
        .filter(|value| !provider_oauth_base_url_is_loopback(value))
}

fn provider_oauth_engine_base_url(state: &AppState) -> String {
    let server_base_url = state.server_base_url();
    normalize_provider_oauth_base_url(&server_base_url)
        .unwrap_or_else(|| "http://127.0.0.1:39731".to_string())
}

fn normalize_provider_oauth_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let parsed = reqwest::Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?;
    let mut out = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    Some(out)
}

fn provider_oauth_base_url_is_loopback(base_url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(base_url.trim()) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}
