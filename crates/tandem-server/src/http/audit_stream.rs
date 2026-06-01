use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use tandem_types::{EngineEvent, RequestPrincipal, TenantContext};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::AppState;

pub(crate) async fn audit_stream(
    State(state): State<AppState>,
    Extension(principal): Extension<RequestPrincipal>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Response {
    if !audit_admin_allowed(&principal) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Admin capability required",
                "code": "AUDIT_ADMIN_REQUIRED"
            })),
        )
            .into_response();
    }

    let rx = state.event_bus.subscribe();
    let stream_tenant = tenant_context.clone();
    let stream = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) if audit_event_matches_tenant(&event, &stream_tenant) => {
            audit_event_to_stream_record(&event).map(|record| {
                let line =
                    serde_json::to_string(&record).unwrap_or_else(|_| "{}".to_string()) + "\n";
                Ok::<Bytes, Infallible>(Bytes::from(line))
            })
        }
        Ok(_) => None,
        Err(_) => None,
    });

    let mut response = Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    response
}

fn audit_admin_allowed(principal: &RequestPrincipal) -> bool {
    matches!(principal.source.as_str(), "api_token" | "control_panel")
}

fn audit_event_matches_tenant(event: &EngineEvent, tenant: &TenantContext) -> bool {
    let event_org = event
        .properties
        .get("org_id")
        .or_else(|| event.properties.get("orgID"))
        .or_else(|| event.properties.get("organization_id"))
        .and_then(Value::as_str);
    let event_workspace = event
        .properties
        .get("workspace_id")
        .or_else(|| event.properties.get("workspaceID"))
        .and_then(Value::as_str);

    if let Some(org_id) = event_org {
        if org_id != tenant.org_id {
            return false;
        }
    }
    if let Some(workspace_id) = event_workspace {
        if workspace_id != tenant.workspace_id {
            return false;
        }
    }
    true
}

pub(crate) fn audit_event_to_stream_record(event: &EngineEvent) -> Option<Value> {
    match event.event_type.as_str() {
        "tool.effect.recorded" => tool_effect_record(event),
        "approval.decision.recorded" => approval_decision_record(event),
        "channel.capability.changed" => capability_change_record(event),
        "fintech.protected_action.denied" | "fintech.protected_action.approved" => {
            fintech_protected_action_record(event)
        }
        _ => None,
    }
}

fn base_record(event: &EngineEvent, command: &str, result: Value) -> Value {
    json!({
        "event_type": event.event_type,
        "actor_id": event.properties.get("actor_id").and_then(Value::as_str),
        "executed_as": event.properties.get("executed_as").and_then(Value::as_str).unwrap_or("tandem-server"),
        "command": command,
        "workspace": event.properties.get("workspace").and_then(Value::as_str),
        "tool_call_id": event.properties.get("tool_call_id").and_then(Value::as_str),
        "result": result,
        "timestamp": crate::now_ms(),
        "channel": event.properties.get("channel").and_then(Value::as_str),
    })
}

fn tool_effect_record(event: &EngineEvent) -> Option<Value> {
    let record = event.properties.get("record")?;
    let command = record.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let workspace = record
        .pointer("/args_summary/workspace_root")
        .and_then(Value::as_str);
    let mut row = base_record(
        event,
        command,
        json!({
            "phase": record.get("phase"),
            "status": record.get("status"),
            "error": record.get("error"),
        }),
    );
    let obj = row.as_object_mut()?;
    if let Some(workspace) = workspace {
        obj.insert(
            "workspace".to_string(),
            Value::String(workspace.to_string()),
        );
    }
    if let Some(tool_call_id) = record.get("tool_call_id").and_then(Value::as_str) {
        obj.insert(
            "tool_call_id".to_string(),
            Value::String(tool_call_id.to_string()),
        );
    }
    Some(row)
}

fn approval_decision_record(event: &EngineEvent) -> Option<Value> {
    Some(base_record(
        event,
        "approval_decision",
        json!({
            "run_id": event.properties.get("run_id"),
            "node_id": event.properties.get("node_id"),
            "decision": event.properties.get("decision"),
            "reason": event.properties.get("reason"),
        }),
    ))
}

fn capability_change_record(event: &EngineEvent) -> Option<Value> {
    Some(base_record(
        event,
        "capability_change",
        json!({
            "channel": event.properties.get("channel"),
            "user_id": event.properties.get("user_id"),
            "max_tier": event.properties.get("max_tier"),
        }),
    ))
}

fn fintech_protected_action_record(event: &EngineEvent) -> Option<Value> {
    let command = match event.event_type.as_str() {
        "fintech.protected_action.approved" => "fintech_protected_action_approved",
        _ => "fintech_protected_action_denied",
    };
    Some(base_record(
        event,
        command,
        json!({
            "run_id": event.properties.get("runID"),
            "automation_id": event.properties.get("automationID"),
            "tool": event.properties.get("tool"),
            "classification": event.properties.get("classification"),
            "category": event.properties.get("category"),
            "reason": event.properties.get("reason"),
            "approval": event.properties.get("approval"),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_tool_effect_event_to_ndjson_record_shape() {
        let event = EngineEvent::new(
            "tool.effect.recorded",
            json!({
                "record": {
                    "tool": "read",
                    "tool_call_id": "call-1",
                    "phase": "outcome",
                    "status": "succeeded",
                    "args_summary": { "workspace_root": "/workspace/acme" }
                }
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "read");
        assert_eq!(row["workspace"], "/workspace/acme");
        assert_eq!(row["tool_call_id"], "call-1");
    }

    #[test]
    fn maps_capability_change_event_to_audit_record() {
        let event = EngineEvent::new(
            "channel.capability.changed",
            json!({
                "channel": "telegram",
                "user_id": "42",
                "max_tier": "approve"
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "capability_change");
        assert_eq!(row["channel"], "telegram");
        assert_eq!(row["result"]["max_tier"], "approve");
    }

    #[test]
    fn maps_fintech_protected_action_denial_to_audit_record() {
        let event = EngineEvent::new(
            "fintech.protected_action.denied",
            json!({
                "runID": "run-1",
                "automationID": "automation-1",
                "tool": "mcp.bank.release_funds",
                "classification": "requires_approval",
                "category": "money_movement",
                "reason": "approval required"
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "fintech_protected_action_denied");
        assert_eq!(row["result"]["run_id"], "run-1");
        assert_eq!(row["result"]["category"], "money_movement");
    }

    #[test]
    fn maps_fintech_protected_action_approval_to_audit_record() {
        let event = EngineEvent::new(
            "fintech.protected_action.approved",
            json!({
                "runID": "run-1",
                "automationID": "automation-1",
                "tool": "mcp.bank.release_funds",
                "category": "money_movement",
                "approval": {
                    "gate_node_id": "approve_protected_action",
                    "action_hash": "hash-1"
                }
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "fintech_protected_action_approved");
        assert_eq!(row["result"]["approval"]["action_hash"], "hash-1");
    }
}
