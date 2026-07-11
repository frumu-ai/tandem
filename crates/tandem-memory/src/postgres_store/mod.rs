//! PostgreSQL + pgvector implementation of the portable memory contract.

mod read_query;
mod schema;
mod write_mutate;

use std::str::FromStr;

use async_trait::async_trait;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};
use serde::{Deserialize, Serialize};
use tokio_postgres::NoTls;

use crate::store::*;
use crate::types::DEFAULT_EMBEDDING_DIMENSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostgresDistanceMetric {
    Cosine,
    Euclidean,
    InnerProduct,
}

impl PostgresDistanceMetric {
    fn operator(self) -> &'static str {
        match self {
            Self::Cosine => "<=>",
            Self::Euclidean => "<->",
            Self::InnerProduct => "<#>",
        }
    }

    fn from_env(value: &str) -> MemoryStoreResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cosine" => Ok(Self::Cosine),
            "l2" | "euclidean" => Ok(Self::Euclidean),
            "ip" | "inner_product" => Ok(Self::InnerProduct),
            value => Err(MemoryStoreError::invalid(format!(
                "unsupported TANDEM_MEMORY_POSTGRES_DISTANCE: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresMemoryStoreConfig {
    pub url: String,
    pub embedding_dimension: usize,
    pub distance_metric: PostgresDistanceMetric,
    pub max_pool_size: usize,
}

impl PostgresMemoryStoreConfig {
    pub fn from_env() -> MemoryStoreResult<Self> {
        let url = std::env::var("TANDEM_MEMORY_POSTGRES_URL").map_err(|_| {
            MemoryStoreError::invalid(
                "TANDEM_MEMORY_POSTGRES_URL is required for the postgres memory backend",
            )
        })?;
        let embedding_dimension = std::env::var("TANDEM_MEMORY_EMBEDDING_DIMENSION")
            .ok()
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| {
                MemoryStoreError::invalid("TANDEM_MEMORY_EMBEDDING_DIMENSION must be an integer")
            })?
            .unwrap_or(DEFAULT_EMBEDDING_DIMENSION);
        if !(1..=16_000).contains(&embedding_dimension) {
            return Err(MemoryStoreError::invalid(
                "embedding dimension must be between 1 and 16000",
            ));
        }
        let distance_metric = PostgresDistanceMetric::from_env(
            &std::env::var("TANDEM_MEMORY_POSTGRES_DISTANCE")
                .unwrap_or_else(|_| "cosine".to_string()),
        )?;
        let max_pool_size = std::env::var("TANDEM_MEMORY_POSTGRES_POOL_SIZE")
            .ok()
            .map(|value| value.parse::<usize>())
            .transpose()
            .map_err(|_| {
                MemoryStoreError::invalid("TANDEM_MEMORY_POSTGRES_POOL_SIZE must be an integer")
            })?
            .unwrap_or(16)
            .clamp(1, 128);
        Ok(Self {
            url,
            embedding_dimension,
            distance_metric,
            max_pool_size,
        })
    }
}

#[derive(Clone)]
pub struct PostgresMemoryStore {
    pool: Pool,
    embedding_dimension: usize,
    distance_metric: PostgresDistanceMetric,
}

impl std::fmt::Debug for PostgresMemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresMemoryStore")
            .field("embedding_dimension", &self.embedding_dimension)
            .field("distance_metric", &self.distance_metric)
            .finish_non_exhaustive()
    }
}

impl PostgresMemoryStore {
    pub async fn connect(config: PostgresMemoryStoreConfig) -> MemoryStoreResult<Self> {
        let pg_config = tokio_postgres::Config::from_str(&config.url)
            .map_err(|error| store_error("invalid PostgreSQL URL", error, false))?;
        let manager = Manager::from_config(
            pg_config,
            NoTls,
            ManagerConfig {
                recycling_method: RecyclingMethod::Fast,
            },
        );
        let pool = Pool::builder(manager)
            .max_size(config.max_pool_size)
            .runtime(Runtime::Tokio1)
            .build()
            .map_err(|error| store_error("build PostgreSQL pool", error, false))?;
        let store = Self {
            pool,
            embedding_dimension: config.embedding_dimension,
            distance_metric: config.distance_metric,
        };
        store.apply_migrations().await?;
        Ok(store)
    }

    async fn client(&self) -> MemoryStoreResult<deadpool_postgres::Client> {
        self.pool
            .get()
            .await
            .map_err(|error| store_error("acquire PostgreSQL connection", error, true))
    }
}

fn store_error(context: &str, error: impl std::fmt::Display, retryable: bool) -> MemoryStoreError {
    let mut error = MemoryStoreError::new(
        if retryable {
            MemoryStoreErrorKind::Unavailable
        } else {
            MemoryStoreErrorKind::Internal
        },
        format!("{context}: {error}"),
    );
    error.retryable = retryable;
    error
}

fn json_value<T: serde::Serialize>(value: &T) -> MemoryStoreResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| store_error("serialize memory value", error, false))
}

fn from_json<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> MemoryStoreResult<T> {
    serde_json::from_value(value)
        .map_err(|error| store_error("deserialize memory value", error, false))
}

#[async_trait]
impl MemoryStore for PostgresMemoryStore {
    async fn read(
        &self,
        request: MemoryStoreReadRequest,
    ) -> MemoryStoreResult<MemoryStoreReadResult> {
        self.read_impl(request).await
    }

    async fn query(
        &self,
        request: MemoryStoreQueryRequest,
    ) -> MemoryStoreResult<MemoryStoreQueryResult> {
        self.query_impl(request).await
    }

    async fn write(
        &self,
        request: MemoryStoreWriteRequest,
    ) -> MemoryStoreResult<MemoryStoreWriteResult> {
        self.write_impl(request).await
    }

    async fn mutate(
        &self,
        request: MemoryStoreMutationRequest,
    ) -> MemoryStoreResult<MemoryStoreMutationResult> {
        self.mutate_impl(request).await
    }

    async fn batch(
        &self,
        request: MemoryStoreBatchRequest,
    ) -> MemoryStoreResult<MemoryStoreBatchResult> {
        self.batch_impl(request).await
    }

    async fn backend_health(
        &self,
        request: MemoryBackendHealthRequest,
    ) -> MemoryStoreResult<MemoryBackendHealthResult> {
        self.health_impl(request).await
    }

    async fn recover_backend(
        &self,
        request: MemoryBackendRecoveryRequest,
    ) -> MemoryStoreResult<MemoryBackendRecoveryResult> {
        self.recover_impl(request).await
    }

    async fn migration_capabilities(
        &self,
        request: MemoryMigrationCapabilityRequest,
    ) -> MemoryStoreResult<MemoryMigrationCapabilityResult> {
        let mut result = MemoryMigrationCapabilityResult {
            backend: MemoryBackendKind::Postgres,
            apply_mode: MemoryMigrationApplyMode::OnOpen,
            version_introspection: true,
            transactional_apply: true,
            online_apply: false,
            dry_run: false,
            requirements_satisfied: false,
        };
        result.requirements_satisfied = result.satisfies(&request);
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
