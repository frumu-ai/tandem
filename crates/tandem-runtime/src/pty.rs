use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
}

#[derive(Clone)]
struct PtySession {
    id: String,
    output: Arc<RwLock<String>>,
    stdin: Arc<Mutex<ChildStdin>>,
    child: Arc<Mutex<Child>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtyInfo {
    pub id: String,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtySnapshot {
    pub id: String,
    pub output: String,
    pub running: bool,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn list(&self) -> Vec<PtyInfo> {
        let sessions = self.sessions.read().await;
        let mut out = Vec::new();
        for session in sessions.values() {
            let running = session.child.lock().await.id().is_some();
            out.push(PtyInfo {
                id: session.id.clone(),
                running,
            });
        }
        out
    }

    pub async fn create(&self) -> anyhow::Result<String> {
        let mut child = Command::new("powershell")
            .args(["-NoProfile"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("stderr unavailable"))?;

        let id = Uuid::new_v4().to_string();
        let output = Arc::new(RwLock::new(String::new()));
        let output_stdout = output.clone();
        let output_stderr = output.clone();

        tokio::spawn(async move {
            read_stream(output_stdout, stdout).await;
        });
        tokio::spawn(async move {
            read_stream(output_stderr, stderr).await;
        });

        self.sessions.write().await.insert(
            id.clone(),
            PtySession {
                id: id.clone(),
                output,
                stdin: Arc::new(Mutex::new(stdin)),
                child: Arc::new(Mutex::new(child)),
            },
        );

        Ok(id)
    }

    pub async fn write(&self, id: &str, input: &str) -> anyhow::Result<bool> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions.get(id).cloned()
        };
        let Some(session) = session else {
            return Ok(false);
        };
        let mut stdin = session.stdin.lock().await;
        stdin.write_all(input.as_bytes()).await?;
        stdin.flush().await?;
        Ok(true)
    }

    pub async fn snapshot(&self, id: &str) -> Option<PtySnapshot> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions.get(id).cloned()
        }?;
        let output = session.output.read().await.clone();
        let running = session.child.lock().await.id().is_some();
        Some(PtySnapshot {
            id: id.to_string(),
            output,
            running,
        })
    }

    pub async fn read_since(&self, id: &str, offset: usize) -> Option<(String, usize, bool)> {
        let snapshot = self.snapshot(id).await?;
        let bytes = snapshot.output.as_bytes();
        let safe_offset = offset.min(bytes.len());
        let tail = String::from_utf8_lossy(&bytes[safe_offset..]).to_string();
        Some((tail, bytes.len(), snapshot.running))
    }

    pub async fn kill(&self, id: &str) -> anyhow::Result<bool> {
        let session = self.sessions.write().await.remove(id);
        let Some(session) = session else {
            return Ok(false);
        };
        let mut child = session.child.lock().await;
        let _ = child.kill().await;
        Ok(true)
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn read_stream(
    output: Arc<RwLock<String>>,
    mut stream: impl tokio::io::AsyncRead + Unpin + Send + 'static,
) {
    let mut buf = vec![0_u8; 4096];
    loop {
        let read = match stream.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        let chunk = String::from_utf8_lossy(&buf[..read]).to_string();
        let mut out = output.write().await;
        out.push_str(&chunk);
        if out.len() > 200_000 {
            let cut = out.len().saturating_sub(100_000);
            let tail = out.split_off(cut);
            *out = tail;
        }
    }
}
