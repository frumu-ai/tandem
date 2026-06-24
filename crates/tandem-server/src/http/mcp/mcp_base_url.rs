use super::*;

pub(crate) fn mcp_public_base_url_from_config(cfg: &Value) -> Option<String> {
    cfg.get("hosted")
        .and_then(Value::as_object)
        .and_then(|hosted| hosted.get("public_url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
}

pub(crate) fn mcp_public_base_url_from_env() -> Option<String> {
    [
        "TANDEM_CONTROL_PANEL_PUBLIC_URL",
        "HOSTED_CONTROL_PANEL_PUBLIC_URL",
        "HOSTED_PUBLIC_URL",
    ]
    .into_iter()
    .find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
    })
}
