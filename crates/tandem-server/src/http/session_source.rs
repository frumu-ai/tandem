// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::Value;
use tandem_types::Session;

pub(super) fn normalize_session_source_kind(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

pub(super) fn effective_session_source_kind(session: &Session) -> String {
    if let Some(source_kind) = session
        .source_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return normalize_session_source_kind(source_kind);
    }
    if session.title.trim_start().starts_with("Automation ") && session.title.contains(" / ") {
        return "automation_v2".to_string();
    }
    "chat".to_string()
}

pub(super) fn session_with_effective_source_kind(mut session: Session) -> Session {
    if session
        .source_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        session.source_kind = Some(effective_session_source_kind(&session));
    }
    session
}

pub(super) fn retain_sessions_for_source(sessions: &mut Vec<Session>, source: Option<&str>) {
    let Some(source) = source
        .map(normalize_session_source_kind)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    sessions.retain(|session| effective_session_source_kind(session) == source);
}

pub(super) fn apply_created_session_source(
    session: &mut Session,
    source_kind: Option<String>,
    source_metadata: Option<Value>,
) {
    session.source_kind = source_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| Some("chat".to_string()));
    session.source_metadata = source_metadata;
}
