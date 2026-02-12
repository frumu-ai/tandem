use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Message, ModelSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTime {
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub slug: Option<String>,
    pub version: Option<String>,
    pub project_id: Option<String>,
    pub title: String,
    pub directory: String,
    pub time: SessionTime,
    pub model: Option<ModelSpec>,
    pub provider: Option<String>,
    #[serde(default)]
    pub messages: Vec<Message>,
}

impl Session {
    pub fn new(title: Option<String>, directory: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            slug: None,
            version: Some("v1".to_string()),
            project_id: None,
            title: title.unwrap_or_else(|| "New session".to_string()),
            directory: directory.unwrap_or_else(|| ".".to_string()),
            time: SessionTime {
                created: now,
                updated: now,
            },
            model: None,
            provider: None,
            messages: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub parent_id: Option<String>,
    pub title: Option<String>,
    pub directory: Option<String>,
    pub model: Option<ModelSpec>,
    pub provider: Option<String>,
    pub permission: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    #[serde(default)]
    pub parts: Vec<crate::MessagePartInput>,
    pub model: Option<ModelSpec>,
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
}
