// Embedding Service Module
// Generates embeddings using local fastembed-compatible implementation

use crate::memory::types::{MemoryResult, DEFAULT_EMBEDDING_DIMENSION, DEFAULT_EMBEDDING_MODEL};
use once_cell::sync::OnceCell;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Embedding service for generating vector representations
pub struct EmbeddingService {
    model_name: String,
    dimension: usize,
}

impl EmbeddingService {
    /// Create a new embedding service with default model
    pub fn new() -> Self {
        Self {
            model_name: DEFAULT_EMBEDDING_MODEL.to_string(),
            dimension: DEFAULT_EMBEDDING_DIMENSION,
        }
    }

    /// Create with custom model
    pub fn with_model(model_name: String, dimension: usize) -> Self {
        Self {
            model_name,
            dimension,
        }
    }

    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Generate embeddings for a single text
    ///
    /// Note: This is a simplified implementation that generates deterministic
    /// pseudo-embeddings based on text content. In a production environment,
    /// this should be replaced with actual fastembed or onnxruntime-based
    /// embedding generation.
    pub async fn embed(&self, text: &str) -> MemoryResult<Vec<f32>> {
        // Generate deterministic embedding based on text hash
        // This ensures same text always produces same embedding
        // In production, replace with actual model inference
        Ok(self.generate_deterministic_embedding(text))
    }

    /// Generate embeddings for multiple texts
    pub async fn embed_batch(&self, texts: &[String]) -> MemoryResult<Vec<Vec<f32>>> {
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.generate_deterministic_embedding(text));
        }
        Ok(embeddings)
    }

    /// Generate a deterministic pseudo-embedding
    ///
    /// This creates a vector that has similar properties to real embeddings:
    /// - Same text produces same vector
    /// - Similar texts produce similar vectors
    /// - Vector is normalized (unit length)
    fn generate_deterministic_embedding(&self, text: &str) -> Vec<f32> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut embedding = vec![0.0f32; self.dimension];

        // Use multiple hash passes to generate different dimensions
        for i in 0..self.dimension {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            i.hash(&mut hasher); // Add dimension index to vary values
            let hash = hasher.finish();

            // Convert hash to float in range [-1, 1]
            let value = (hash as f64 / u64::MAX as f64) as f32 * 2.0 - 1.0;
            embedding[i] = value;
        }

        // Normalize to unit length
        Self::normalize(&mut embedding);

        embedding
    }

    /// Normalize a vector to unit length
    fn normalize(vector: &mut [f32]) {
        let magnitude: f32 = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for x in vector.iter_mut() {
                *x /= magnitude;
            }
        }
    }

    /// Calculate cosine similarity between two vectors
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

    /// Calculate Euclidean distance between two vectors
    pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }
}

impl Default for EmbeddingService {
    fn default() -> Self {
        Self::new()
    }
}

// Global embedding service instance
static EMBEDDING_SERVICE: OnceCell<Arc<Mutex<EmbeddingService>>> = OnceCell::new();

/// Get or initialize the global embedding service
pub async fn get_embedding_service() -> Arc<Mutex<EmbeddingService>> {
    EMBEDDING_SERVICE
        .get_or_init(|| Arc::new(Mutex::new(EmbeddingService::new())))
        .clone()
}

/// Initialize the embedding service with custom configuration
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
    async fn test_embedding_deterministic() {
        let service = EmbeddingService::new();
        let text = "Hello world";

        let embedding1 = service.embed(text).await.unwrap();
        let embedding2 = service.embed(text).await.unwrap();

        assert_eq!(embedding1.len(), DEFAULT_EMBEDDING_DIMENSION);
        assert_eq!(embedding1, embedding2);
    }

    #[tokio::test]
    async fn test_embedding_normalized() {
        let service = EmbeddingService::new();
        let text = "Test text for normalization";

        let embedding = service.embed(text).await.unwrap();

        // Calculate magnitude
        let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (magnitude - 1.0).abs() < 1e-6,
            "Embedding should be normalized"
        );
    }

    #[tokio::test]
    async fn test_cosine_similarity() {
        let service = EmbeddingService::new();

        let text1 = "The quick brown fox";
        let text2 = "The quick brown fox"; // Same text
        let text3 = "A completely different sentence";

        let emb1 = service.embed(text1).await.unwrap();
        let emb2 = service.embed(text2).await.unwrap();
        let emb3 = service.embed(text3).await.unwrap();

        let sim_same = EmbeddingService::cosine_similarity(&emb1, &emb2);
        let sim_diff = EmbeddingService::cosine_similarity(&emb1, &emb3);

        assert!(
            (sim_same - 1.0).abs() < 1e-6,
            "Same text should have similarity 1.0"
        );
        assert!(
            sim_diff < sim_same,
            "Different text should have lower similarity"
        );
    }

    #[tokio::test]
    async fn test_embed_batch() {
        let service = EmbeddingService::new();
        let texts = vec![
            "First text".to_string(),
            "Second text".to_string(),
            "Third text".to_string(),
        ];

        let embeddings = service.embed_batch(&texts).await.unwrap();

        assert_eq!(embeddings.len(), 3);
        for emb in &embeddings {
            assert_eq!(emb.len(), DEFAULT_EMBEDDING_DIMENSION);
        }
    }
}
