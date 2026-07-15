// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
#[test]
fn default_model_spec_from_effective_config_reads_default_route() {
    let cfg = serde_json::json!({
        "default_provider": "openrouter",
        "providers": {
            "openrouter": {
                "default_model": "google/gemini-3-flash-preview"
            }
        }
    });
    let spec = default_model_spec_from_effective_config(&cfg).expect("default model spec");
    assert_eq!(spec.provider_id, "openrouter");
    assert_eq!(spec.model_id, "google/gemini-3-flash-preview");
}

#[test]
fn default_model_spec_from_effective_config_returns_none_when_incomplete() {
    let missing_provider = serde_json::json!({
        "providers": {
            "openrouter": {
                "default_model": "google/gemini-3-flash-preview"
            }
        }
    });
    assert!(default_model_spec_from_effective_config(&missing_provider).is_none());

    let missing_model = serde_json::json!({
        "default_provider": "openrouter",
        "providers": {
            "openrouter": {}
        }
    });
    assert!(default_model_spec_from_effective_config(&missing_model).is_none());
}

#[test]
fn derive_status_index_update_for_run_started() {
    let event = EngineEvent::new(
        "session.run.started",
        serde_json::json!({
            "sessionID": "s-1",
            "runID": "r-1"
        }),
    );
    let update = derive_status_index_update(&event).expect("update");
    assert_eq!(update.key, "run/s-1/status");
    assert_eq!(
        update.value.get("state").and_then(|v| v.as_str()),
        Some("running")
    );
    assert_eq!(
        update.value.get("phase").and_then(|v| v.as_str()),
        Some("run")
    );
}

#[test]
fn derive_status_index_update_for_tool_invocation() {
    let event = EngineEvent::new(
        "message.part.updated",
        serde_json::json!({
            "sessionID": "s-2",
            "runID": "r-2",
            "part": { "type": "tool-invocation", "tool": "todo_write" }
        }),
    );
    let update = derive_status_index_update(&event).expect("update");
    assert_eq!(update.key, "run/s-2/status");
    assert_eq!(
        update.value.get("phase").and_then(|v| v.as_str()),
        Some("tool")
    );
    assert_eq!(
        update.value.get("toolActive").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        update.value.get("tool").and_then(|v| v.as_str()),
        Some("todo_write")
    );
}

#[test]
fn derive_status_index_update_for_failed_write_includes_recovery_snapshot() {
    let event = EngineEvent::new(
        "message.part.updated",
        serde_json::json!({
            "sessionID": "s-3",
            "runID": "r-3",
            "part": {
                "id": "call_stream_1",
                "type": "tool",
                "state": "failed",
                "tool": "write",
                "args": {
                    "path": "game.html",
                    "content": "<html>draft</html>"
                },
                "error": "WRITE_ARGS_EMPTY_FROM_PROVIDER"
            }
        }),
    );
    let update = derive_status_index_update(&event).expect("update");
    assert_eq!(update.key, "run/s-3/status");
    assert_eq!(
        update.value.get("phase").and_then(|v| v.as_str()),
        Some("run")
    );
    assert_eq!(
        update.value.get("toolActive").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        update.value.get("tool").and_then(|v| v.as_str()),
        Some("write")
    );
    assert_eq!(
        update.value.get("toolState").and_then(|v| v.as_str()),
        Some("failed")
    );
    assert_eq!(
        update.value.get("toolError").and_then(|v| v.as_str()),
        Some("WRITE_ARGS_EMPTY_FROM_PROVIDER")
    );
    assert_eq!(
        update.value.get("toolCallID").and_then(|v| v.as_str()),
        Some("call_stream_1")
    );
    let preview = update
        .value
        .get("toolArgsPreview")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(preview.contains("game.html"));
    assert!(preview.contains("<html>draft</html>"));
}
