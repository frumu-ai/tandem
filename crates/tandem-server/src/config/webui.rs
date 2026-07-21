// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebUiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_web_ui_prefix")]
    pub path_prefix: String,
}

const DEFAULT_WEB_UI_PREFIX: &str = "/admin";

pub fn normalize_web_ui_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return DEFAULT_WEB_UI_PREFIX.to_string();
    }
    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    let normalized = with_leading.trim_end_matches('/');
    if web_ui_prefix_is_reserved(normalized) && web_ui_prefix_has_safe_segments(normalized) {
        normalized.to_string()
    } else {
        tracing::warn!(
            requested_prefix = %trimmed,
            fallback_prefix = DEFAULT_WEB_UI_PREFIX,
            "rejected Web UI prefix outside the reserved UI namespace"
        );
        DEFAULT_WEB_UI_PREFIX.to_string()
    }
}

fn web_ui_prefix_is_reserved(prefix: &str) -> bool {
    prefix == DEFAULT_WEB_UI_PREFIX || prefix == "/ui" || prefix.starts_with("/ui/")
}

fn web_ui_prefix_has_safe_segments(prefix: &str) -> bool {
    !prefix.contains("//")
        && !prefix.contains(['\\', '%', '?', '#'])
        && prefix.split('/').skip(1).all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        })
}

fn default_web_ui_prefix() -> String {
    DEFAULT_WEB_UI_PREFIX.to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_web_ui_prefix;

    #[test]
    fn web_ui_prefix_is_confined_to_reserved_namespaces() {
        assert_eq!(normalize_web_ui_prefix("/admin"), "/admin");
        assert_eq!(normalize_web_ui_prefix("/ui"), "/ui");
        assert_eq!(normalize_web_ui_prefix("/ui/operators"), "/ui/operators");
        assert_eq!(normalize_web_ui_prefix("/auth"), "/admin");
        assert_eq!(normalize_web_ui_prefix("/global/health"), "/admin");
        assert_eq!(normalize_web_ui_prefix("/ui/../global"), "/admin");
        assert_eq!(normalize_web_ui_prefix("/ui%2fglobal"), "/admin");
        assert_eq!(normalize_web_ui_prefix("/ui\\global"), "/admin");
    }
}
