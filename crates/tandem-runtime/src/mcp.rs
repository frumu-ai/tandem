use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub transport: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct McpRegistry {
    servers: Arc<RwLock<HashMap<String, McpServer>>>,
    processes: Arc<Mutex<HashMap<String, Child>>>,
    state_file: Arc<PathBuf>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::new_with_state_file(resolve_state_file())
    }

    pub fn new_with_state_file(state_file: PathBuf) -> Self {
        let loaded = load_state(&state_file)
            .into_iter()
            .map(|(k, mut v)| {
                v.connected = false;
                v.pid = None;
                (k, v)
            })
            .collect::<HashMap<_, _>>();
        Self {
            servers: Arc::new(RwLock::new(loaded)),
            processes: Arc::new(Mutex::new(HashMap::new())),
            state_file: Arc::new(state_file),
        }
    }

    pub async fn list(&self) -> HashMap<String, McpServer> {
        self.servers.read().await.clone()
    }

    pub async fn add(&self, name: String, transport: String) {
        self.servers.write().await.insert(
            name.clone(),
            McpServer {
                name,
                transport,
                connected: false,
                pid: None,
                last_error: None,
            },
        );
        self.persist_state().await;
    }

    pub async fn connect(&self, name: &str) -> bool {
        let transport = {
            let servers = self.servers.read().await;
            let Some(server) = servers.get(name) else {
                return false;
            };
            server.transport.clone()
        };

        if let Some(command_text) = parse_stdio_transport(&transport) {
            match spawn_stdio_process(command_text).await {
                Ok(child) => {
                    let pid = child.id();
                    self.processes.lock().await.insert(name.to_string(), child);
                    let mut servers = self.servers.write().await;
                    if let Some(server) = servers.get_mut(name) {
                        server.connected = true;
                        server.pid = pid;
                        server.last_error = None;
                    }
                    drop(servers);
                    self.persist_state().await;
                    true
                }
                Err(err) => {
                    let mut servers = self.servers.write().await;
                    if let Some(server) = servers.get_mut(name) {
                        server.connected = false;
                        server.pid = None;
                        server.last_error = Some(err);
                    }
                    drop(servers);
                    self.persist_state().await;
                    false
                }
            }
        } else {
            let mut servers = self.servers.write().await;
            if let Some(server) = servers.get_mut(name) {
                server.connected = true;
                server.pid = None;
                server.last_error = None;
            }
            drop(servers);
            self.persist_state().await;
            true
        }
    }

    pub async fn disconnect(&self, name: &str) -> bool {
        if let Some(mut child) = self.processes.lock().await.remove(name) {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.get_mut(name) {
            server.connected = false;
            server.pid = None;
            drop(servers);
            self.persist_state().await;
            return true;
        }
        false
    }

    async fn persist_state(&self) {
        let snapshot = self.servers.read().await.clone();
        if let Some(parent) = self.state_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Ok(payload) = serde_json::to_string_pretty(&snapshot) {
            let _ = tokio::fs::write(self.state_file.as_path(), payload).await;
        }
    }
}

impl Default for McpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_state_file() -> PathBuf {
    if let Ok(path) = std::env::var("TANDEM_MCP_REGISTRY") {
        return PathBuf::from(path);
    }
    PathBuf::from(".tandem").join("mcp_servers.json")
}

fn load_state(path: &Path) -> HashMap<String, McpServer> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str::<HashMap<String, McpServer>>(&raw).unwrap_or_default()
}

fn parse_stdio_transport(transport: &str) -> Option<&str> {
    transport.strip_prefix("stdio:").map(str::trim)
}

async fn spawn_stdio_process(command_text: &str) -> Result<Child, String> {
    if command_text.is_empty() {
        return Err("Missing stdio command".to_string());
    }
    #[cfg(windows)]
    let mut command = {
        let mut cmd = Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", command_text]);
        cmd
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut cmd = Command::new("sh");
        cmd.args(["-lc", command_text]);
        cmd
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command.spawn().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn add_connect_disconnect_non_stdio_server() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add("example".to_string(), "sse:https://example.com".to_string())
            .await;
        assert!(registry.connect("example").await);
        let listed = registry.list().await;
        assert!(listed.get("example").map(|s| s.connected).unwrap_or(false));
        assert!(registry.disconnect("example").await);
    }
}
