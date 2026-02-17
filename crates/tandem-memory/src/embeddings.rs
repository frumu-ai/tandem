// Embedding Service Module
// Generates embeddings using local fastembed implementation.

use crate::types::{
    MemoryError, MemoryResult, DEFAULT_EMBEDDING_DIMENSION, DEFAULT_EMBEDDING_MODEL,
};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Embedding service for generating vector representations.
pub struct EmbeddingService {
    model_name: String,
    dimension: usize,
    model: Option<TextEmbedding>,
    disabled_reason: Option<String>,
}

impl EmbeddingService {
    /// Create a new embedding service with default model.
    pub fn new() -> Self {
        Self::with_model(
            DEFAULT_EMBEDDING_MODEL.to_string(),
            DEFAULT_EMBEDDING_DIMENSION,
        )
    }

    /// Create with custom model.
    pub fn with_model(model_name: String, dimension: usize) -> Self {
        let (model, disabled_reason) = Self::init_model(&model_name);

        if let Some(reason) = &disabled_reason {
            tracing::warn!(
                target: "tandem.memory",
                "Embeddings disabled: model={} reason={}",
                model_name,
                reason
            );
        } else {
            tracing::info!(
                target: "tandem.memory",
                "Embeddings enabled: model={} dimension={}",
                model_name,
                dimension
            );
        }

        Self {
            model_name,
            dimension,
            model,
            disabled_reason,
        }
    }

    fn init_model(model_name: &str) -> (Option<TextEmbedding>, Option<String>) {
        let Some(parsed_model) = Self::parse_model_id(model_name) else {
            return (
                None,
                Some(format!(
                    "unsupported embedding model id '{}'; supported: {}",
                    model_name, DEFAULT_EMBEDDING_MODEL
                )),
            );
        };

        let cache_dir = resolve_embedding_cache_dir();
        let options = InitOptions::new(parsed_model).with_cache_dir(cache_dir.clone());

        tracing::info!(
            target: "tandem.memory",
            "Initializing embeddings with cache dir: {}",
            cache_dir.display()
        );

        match TextEmbedding::try_new(options) {
            Ok(model) => (Some(model), None),
            Err(err) => (
                None,
                Some(format!(
                    "failed to initialize embedding model '{}': {}",
                    model_name, err
                )),
            ),
        }
    }

    fn parse_model_id(model_name: &str) -> Option<EmbeddingModel> {
        match model_name.trim().to_ascii_lowercase().as_str() {
            "all-minilm-l6-v2" | "all_minilm_l6_v2" => Some(EmbeddingModel::AllMiniLML6V2),
            _ => None,
        }
    }

    /// Get the embedding dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Get the model name.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Returns whether semantic embeddings are currently available.
    pub fn is_available(&self) -> bool {
        self.model.is_some()
    }

    /// Returns disabled reason if embeddings are unavailable.
    pub fn disabled_reason(&self) -> Option<&str> {
        self.disabled_reason.as_deref()
    }

    fn unavailable_error(&self) -> MemoryError {
        let reason = self
            .disabled_reason
            .as_deref()
            .unwrap_or("embedding backend unavailable");
        MemoryError::Embedding(format!("embeddings disabled: {reason}"))
    }

    fn ensure_dimension(&self, embedding: &[f32]) -> MemoryResult<()> {
        if embedding.len() != self.dimension {
            return Err(MemoryError::Embedding(format!(
                "embedding dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.len()
            )));
        }
        Ok(())
    }

    /// Generate embeddings for a single text.
    pub async fn embed(&self, text: &str) -> MemoryResult<Vec<f32>> {
        let Some(model) = self.model.as_ref() else {
            return Err(self.unavailable_error());
        };

        let mut embeddings = model
            .embed(vec![text.to_string()], None)
            .map_err(|e| MemoryError::Embedding(e.to_string()))?;
        let embedding = embeddings
            .pop()
            .ok_or_else(|| MemoryError::Embedding("no embedding generated".to_string()))?;
        self.ensure_dimension(&embedding)?;
        Ok(embedding)
    }

    /// Generate embeddings for multiple texts.
    pub async fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Vec<f32>>> {
        let Some(model) = self.model.as_ref() else {
            return Err(self.unavailable_error());
        };

        let embeddings = model
            .embed(texts.to_vec(), None)
            .map_err(|e| MemoryError::Embedding(e.to_string()))?;

        for embedding in &embeddings {
            self.ensure_dimension(embedding)?;
        }

        Ok(embeddings)
    }

    /// Calculate cosine similarity between two vectors.
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if magnitude_a == 0.0 || magnitude_b == 0.0 {
            0.0
        } else {
            dot_product / (magnitude_a * magnitude_b)
        }
    }

    /// Calculate Euclidean distance between two vectors.
    pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }
}

fn resolve_embedding_cache_dir() -> PathBuf {
    if let Ok(explicit) = std::env::var("FASTEMBED_CACHE_DIR") {
        let explicit_path = PathBuf::from(explicit);
        if let Err(err) = std::fs::create_dir_all(&explicit_path) {
            tracing::warn!(
                target: "tandem.memory",
                "Failed to create FASTEMBED_CACHE_DIR {:?}: {}",
                explicit_path,
                err
            );
        }
        return explicit_path;
    }

    let base = dirs::data_local_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(std::env::temp_dir);
    let cache_dir = base.join("tandem").join("fastembed");

    if let Err(err) = std::fs::create_dir_all(&cache_dir) {
        tracing::warn!(
            target: "tandem.memory",
            "Failed to create embedding cache directory {:?}: {}",
            cache_dir,
            err
        );
    }

    cache_dir
}

impl Default for EmbeddingService {
    fn default() -> Self {
        Self::new()
    }
}

// Global embedding service instance.
static EMBEDDING_SERVICE: OnceCell<Arc<Mutex<EmbeddingService>>> = OnceCell::new();

/// Get or initialize the global embedding service.
pub async fn get_embedding_service() -> Arc<Mutex<EmbeddingService>> {
    EMBEDDING_SERVICE
        .get_or_init(|| Arc::new(Mutex::new(EmbeddingService::new())))
        .clone()
}

/// Initialize the embedding service with custom configuration.
pub fn init_embedding_service(model_name: Option<String>, dimension: Option<usize>) {
    let service = if let (Some(name), Some(dim)) = (model_name, dimension) {
        EmbeddingService::with_model(name, dim)
    } else {
        EmbeddingService::new()
    };

    let _ = EMBEDDING_SERVICE.set(Arc::new(Mutex::new(service)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedding_dimension_or_unavailable() {
        let service = EmbeddingService::new();

        if !service.is_available() {
            let err = service.embed("Hello world").await.unwrap_err();
            assert!(err.to_string().contains("embeddings disabled"));
            return;
        }

        let embedding = service.embed("Hello world").await.unwrap();
        assert_eq!(embedding.len(), DEFAULT_EMBEDDING_DIMENSION);
    }

    #[tokio::test]
    async fn test_embed_batch_or_unavailable() {
        let service = EmbeddingService::new();
        let texts = vec![
            "First text".to_string(),
            "Second text".to_string(),
            "Third text".to_string(),
        ];

        let result = service.embed_batch(&texts).await;
        if !service.is_available() {
            assert!(result.is_err());
            return;
        }

        let embeddings = result.unwrap();
        assert_eq!(embeddings.len(), 3);
        for emb in &embeddings {
            assert_eq!(emb.len(), DEFAULT_EMBEDDING_DIMENSION);
        }
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0f32, 0.0, 0.0];
        let b = vec![1.0f32, 0.0, 0.0];
        let c = vec![0.0f32, 1.0, 0.0];

        let sim_same = EmbeddingService::cosine_similarity(&a, &b);
        let sim_orthogonal = EmbeddingService::cosine_similarity(&a, &c);

        assert!((sim_same - 1.0).abs() < 1e-6);
        assert!(sim_orthogonal.abs() < 1e-6);
    }
}
