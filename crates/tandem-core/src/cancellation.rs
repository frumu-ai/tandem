use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Default)]
pub struct CancellationRegistry {
    state: Arc<RwLock<CancellationState>>,
}

#[derive(Default)]
struct CancellationState {
    tokens: HashMap<String, CancellationToken>,
    deferred: HashSet<String>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn create(&self, session_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        let mut state = self.state.write().await;
        if state.deferred.remove(session_id) {
            token.cancel();
        }
        state.tokens.insert(session_id.to_string(), token.clone());
        token
    }

    pub async fn get(&self, session_id: &str) -> Option<CancellationToken> {
        self.state.read().await.tokens.get(session_id).cloned()
    }

    pub async fn cancel(&self, session_id: &str) -> bool {
        let token = self.state.read().await.tokens.get(session_id).cloned();
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn cancel_or_defer(&self, session_id: &str) -> bool {
        let mut state = self.state.write().await;
        if let Some(token) = state.tokens.get(session_id).cloned() {
            token.cancel();
        } else {
            state.deferred.insert(session_id.to_string());
        }
        true
    }

    pub async fn remove(&self, session_id: &str) {
        let mut state = self.state.write().await;
        state.tokens.remove(session_id);
        state.deferred.remove(session_id);
    }

    pub async fn cancel_all(&self) -> usize {
        let tokens = self
            .state
            .read()
            .await
            .tokens
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
