use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{watch, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tandem_types::EngineEvent;

use crate::event_bus::EventBus;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    pub permission: String,
    pub pattern: String,
    pub action: PermissionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sessionID")]
    pub session_id: Option<String>,
    pub permission: String,
    pub pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "argsSource")]
    pub args_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "argsIntegrity")]
    pub args_integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionArgsContext {
    #[serde(rename = "argsSource")]
    pub args_source: String,
    #[serde(rename = "argsIntegrity")]
    pub args_integrity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

#[derive(Clone)]
pub struct PermissionManager {
    requests: Arc<RwLock<HashMap<String, PermissionRequest>>>,
    rules: Arc<RwLock<Vec<PermissionRule>>>,
    waiters: Arc<RwLock<HashMap<String, watch::Sender<Option<String>>>>>,
    event_bus: EventBus,
}

impl PermissionManager {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            rules: Arc::new(RwLock::new(Vec::new())),
            waiters: Arc::new(RwLock::new(HashMap::new())),
            event_bus,
        }
    }

    pub async fn evaluate(&self, permission: &str, pattern: &str) -> PermissionAction {
        let permission = normalize_permission_alias(permission);
        let pattern = normalize_permission_alias(pattern);
        let rules = self.rules.read().await;
        if let Some(rule) = rules.iter().rev().find(|rule| {
            normalize_permission_alias(&rule.permission) == permission
                && wildcard_matches(&normalize_permission_alias(&rule.pattern), &pattern)
        }) {
            return rule.action.clone();
        }
        PermissionAction::Ask
    }

    pub async fn ask_for_session(
        &self,
        session_id: Option<&str>,
        tool: &str,
        args: Value,
    ) -> PermissionRequest {
        self.ask_for_session_with_context(session_id, tool, args, None)
            .await
    }

    pub async fn ask_for_session_with_context(
        &self,
        session_id: Option<&str>,
        tool: &str,
        args: Value,
        context: Option<PermissionArgsContext>,
    ) -> PermissionRequest {
        let req = PermissionRequest {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.map(ToString::to_string),
            permission: tool.to_string(),
            pattern: tool.to_string(),
            tool: Some(tool.to_string()),
            args: Some(args.clone()),
            args_source: context.as_ref().map(|c| c.args_source.clone()),
            args_integrity: context.as_ref().map(|c| c.args_integrity.clone()),
            query: context.as_ref().and_then(|c| c.query.clone()),
            status: "pending".to_string(),
        };
        let (tx, _rx) = watch::channel(None);
        self.requests
            .write()
            .await
            .insert(req.id.clone(), req.clone());
        self.waiters.write().await.insert(req.id.clone(), tx);
        self.event_bus.publish(EngineEvent::new(
            "permission.asked",
            json!({
                "sessionID": session_id.unwrap_or_default(),
                "requestID": req.id,
                "tool": tool,
                "args": args,
                "argsSource": req.args_source,
                "argsIntegrity": req.args_integrity,
                "query": req.query
            }),
        ));
        req
    }

    pub async fn ask(&self, permission: &str, pattern: &str) -> PermissionRequest {
        let tool = if permission.is_empty() {
            pattern.to_string()
        } else {
            permission.to_string()
        };
        self.ask_for_session(None, &tool, json!({})).await
    }

    pub async fn list(&self) -> Vec<PermissionRequest> {
        self.requests.read().await.values().cloned().collect()
    }

    pub async fn list_rules(&self) -> Vec<PermissionRule> {
        self.rules.read().await.clone()
    }

    pub async fn reply(&self, id: &str, reply: &str) -> bool {
        let (permission, pattern) = {
            let mut requests = self.requests.write().await;
            let Some(req) = requests.get_mut(id) else {
                return false;
            };
            req.status = reply.to_string();
            (req.permission.clone(), req.pattern.clone())
        };

        if matches!(reply, "always" | "allow") {
            self.rules.write().await.push(PermissionRule {
                id: Uuid::new_v4().to_string(),
                permission,
                pattern,
                action: PermissionAction::Allow,
            });
        } else if matches!(reply, "reject" | "deny") {
            self.rules.write().await.push(PermissionRule {
                id: Uuid::new_v4().to_string(),
                permission,
                pattern,
                action: PermissionAction::Deny,
            });
        }

        self.event_bus.publish(EngineEvent::new(
            "permission.replied",
            json!({"requestID": id, "reply": reply}),
        ));
        if let Some(waiter) = self.waiters.read().await.get(id).cloned() {
            let _ = waiter.send(Some(reply.to_string()));
        }
        true
    }

    pub async fn wait_for_reply(&self, id: &str, cancel: CancellationToken) -> Option<String> {
        let mut rx = {
            let waiters = self.waiters.read().await;
            waiters.get(id).map(|tx| tx.subscribe())?
        };
        let immediate = { rx.borrow().clone() };
        if let Some(reply) = immediate {
            self.waiters.write().await.remove(id);
            return Some(reply);
        }
        let waited: Option<String> = tokio::select! {
            _ = cancel.cancelled() => None,
            changed = rx.changed() => {
                if changed.is_ok() {
                    let updated = { rx.borrow().clone() };
                    updated
                } else {
                    None
                }
            }
        };
        self.waiters.write().await.remove(id);
        waited
    }
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let mut remaining = value;
    let mut is_first = true;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        if is_first {
            if let Some(stripped) = remaining.strip_prefix(part) {
                remaining = stripped;
            } else {
                return false;
            }
            is_first = false;
            continue;
        }
        if let Some(index) = remaining.find(part) {
            remaining = &remaining[index + part.len()..];
        } else {
            return false;
        }
    }
    pattern.ends_with('*') || remaining.is_empty()
}

fn normalize_permission_alias(input: &str) -> String {
    match input.trim().to_lowercase().replace('-', "_").as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_for_reply_returns_user_response() {
        let bus = EventBus::new();
        let manager = PermissionManager::new(bus);
        let request = manager
            .ask_for_session(Some("ses_1"), "bash", json!({"command":"echo hi"}))
            .await;

        let id = request.id.clone();
        let manager_clone = manager.clone();
        tokio::spawn(async move {
            let _ = manager_clone.reply(&id, "allow").await;
        });

        let cancel = CancellationToken::new();
        let reply = manager.wait_for_reply(&request.id, cancel).await;
        assert_eq!(reply.as_deref(), Some("allow"));
    }

    #[tokio::test]
    async fn permission_asked_event_contains_tool_and_args() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let manager = PermissionManager::new(bus);

        let _ = manager
            .ask_for_session(Some("ses_1"), "read", json!({"path":"README.md"}))
            .await;
        let event = rx.recv().await.expect("event");
        assert_eq!(event.event_type, "permission.asked");
        assert_eq!(
            event
                .properties
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "read"
        );
        assert_eq!(
            event
                .properties
                .get("args")
                .and_then(|v| v.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "README.md"
        );
    }

    #[tokio::test]
    async fn permission_asked_event_includes_args_integrity_context() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let manager = PermissionManager::new(bus);

        let _ = manager
            .ask_for_session_with_context(
                Some("ses_1"),
                "websearch",
                json!({"query":"meaning of life"}),
                Some(PermissionArgsContext {
                    args_source: "inferred_from_user".to_string(),
                    args_integrity: "recovered".to_string(),
                    query: Some("meaning of life".to_string()),
                }),
            )
            .await;

        let event = rx.recv().await.expect("event");
        assert_eq!(event.event_type, "permission.asked");
        assert_eq!(
            event
                .properties
                .get("argsSource")
                .and_then(|v| v.as_str()),
            Some("inferred_from_user")
        );
        assert_eq!(
            event
                .properties
                .get("argsIntegrity")
                .and_then(|v| v.as_str()),
            Some("recovered")
        );
        assert_eq!(
            event.properties.get("query").and_then(|v| v.as_str()),
            Some("meaning of life")
        );
    }

    #[tokio::test]
    async fn evaluate_todo_aliases_as_same_permission() {
        let bus = EventBus::new();
        let manager = PermissionManager::new(bus);
        manager.rules.write().await.push(PermissionRule {
            id: Uuid::new_v4().to_string(),
            permission: "todowrite".to_string(),
            pattern: "todowrite".to_string(),
            action: PermissionAction::Allow,
        });

        let action = manager.evaluate("todo_write", "todo_write").await;
        assert!(matches!(action, PermissionAction::Allow));
    }
}
