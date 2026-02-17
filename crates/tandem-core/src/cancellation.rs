use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct CancellationRegistry {
    tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create(&self, session_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens
            .write()
            .await
            .insert(session_id.to_string(), token.clone());
        token
    }

    pub async fn get(&self, session_id: &str) -> Option<CancellationToken> {
        self.tokens.read().await.get(session_id).cloned()
    }

    pub async fn cancel(&self, session_id: &str) -> bool {
        let token = self.tokens.read().await.get(session_id).cloned();
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn remove(&self, session_id: &str) {
        self.tokens.write().await.remove(session_id);
    }

    pub async fn cancel_all(&self) -> usize {
        let tokens = self
            .tokens
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let count = tokens.len();
        for token in tokens {
            token.cancel();
        }
        count
    }
}
