use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct EngineClient {
    base_url: String,
    client: Client,
    api_key: Option<String>, // If we need it later
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Session {
    pub id: String,

    pub name: Option<String>,
    // Add other fields as needed
}

impl EngineClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
            api_key: None,
        }
    }

    pub async fn check_health(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        // Note: The actual endpoint might be /global/health or just /health depending on server
        // Let's assume /health for now or check server code
        let resp = self.client.get(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let url = format!("{}/api/session", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let sessions = resp.json::<Vec<Session>>().await?;
        Ok(sessions)
    }

    pub async fn create_session(&self, title: Option<String>) -> Result<Session> {
        use tandem_types::CreateSessionRequest;
        let url = format!("{}/api/session", self.base_url);
        let req = CreateSessionRequest {
            parent_id: None,
            title,
            directory: None,
            model: None,
            provider: None,
            permission: None,
        };

        let resp = self.client.post(&url).json(&req).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }
}
