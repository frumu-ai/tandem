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
    pub status: String,
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
        let rules = self.rules.read().await;
        if let Some(rule) = rules
            .iter()
            .rev()
            .find(|rule| rule.permission == permission && wildcard_matches(&rule.pattern, pattern))
        {
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
        let req = PermissionRequest {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.map(ToString::to_string),
            permission: tool.to_string(),
            pattern: tool.to_string(),
            tool: Some(tool.to_string()),
            args: Some(args.clone()),
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
                "args": args
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
}
