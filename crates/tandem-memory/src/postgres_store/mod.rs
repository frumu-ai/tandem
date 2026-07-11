//! PostgreSQL + pgvector implementation of the portable memory contract.

mod read_query;
mod schema;
mod write_mutate;

use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};
use serde::{Deserialize, Serialize};
use tokio_postgres::NoTls;

use crate::crypto::MemoryCryptoProvider;
use crate::decrypt_broker::MemoryDecryptBrokerConfig;
use crate::envelope::{MemoryEnvelopeAuthority, MemoryEnvelopeMetadata, MemoryKeyScope};
use crate::store::*;
use crate::types::DEFAULT_EMBEDDING_DIMENSION;

type EncodedPayload = (
    Option<serde_json::Value>,
    Option<String>,
    Option<serde_json::Value>,
    Option<String>,
    Option<String>,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostgresDistanceMetric {
    Cosine,
    Euclidean,
    InnerProduct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostgresSearchSurfaceMode {
    PlaintextPgvector,
    EncryptedRerank,
    Disabled,
}

impl PostgresSearchSurfaceMode {
    fn from_env() -> MemoryStoreResult<Self> {
        let required = std::env::var("TANDEM_MEMORY_ENCRYPTION_REQUIRED")
            .ok()
            .is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            });
        let configured = std::env::var("TANDEM_MEMORY_SEARCH_SURFACE_MODE").ok();
        let mode = match configured
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            None | Some("") if required => Self::EncryptedRerank,
            None | Some("") | Some("plaintext_pgvector") | Some("plaintext") => {
                Self::PlaintextPgvector
            }
            Some("encrypted_rerank") | Some("encrypted") => Self::EncryptedRerank,
            Some("disabled") => Self::Disabled,
            Some(value) => {
                return Err(MemoryStoreError::invalid(format!(
                    "unsupported TANDEM_MEMORY_SEARCH_SURFACE_MODE: {value}"
                )))
            }
        };
        if required && mode == Self::PlaintextPgvector {
            return Err(MemoryStoreError::invalid(
                "hosted encryption forbids plaintext PostgreSQL search surfaces; use encrypted_rerank or disabled",
            ));
        }
        Ok(mode)
    }
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
    pub pool_wait_timeout: Duration,
    pub search_surface_mode: PostgresSearchSurfaceMode,
    pub rerank_candidate_limit: i64,
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
        let pool_wait_timeout = Duration::from_millis(
            std::env::var("TANDEM_MEMORY_POSTGRES_POOL_WAIT_TIMEOUT_MS")
                .ok()
                .map(|value| value.parse::<u64>())
                .transpose()
                .map_err(|_| {
                    MemoryStoreError::invalid(
                        "TANDEM_MEMORY_POSTGRES_POOL_WAIT_TIMEOUT_MS must be an integer",
                    )
                })?
                .unwrap_or(5_000)
                .clamp(10, 120_000),
        );
        let search_surface_mode = PostgresSearchSurfaceMode::from_env()?;
        let rerank_candidate_limit = std::env::var("TANDEM_MEMORY_POSTGRES_RERANK_CANDIDATES")
            .ok()
            .map(|value| value.parse::<i64>())
            .transpose()
            .map_err(|_| {
                MemoryStoreError::invalid(
                    "TANDEM_MEMORY_POSTGRES_RERANK_CANDIDATES must be an integer",
                )
            })?
            .unwrap_or(1000)
            .clamp(1, 10_000);
        Ok(Self {
            url,
            embedding_dimension,
            distance_metric,
            max_pool_size,
            pool_wait_timeout,
            search_surface_mode,
            rerank_candidate_limit,
        })
    }
}

#[derive(Clone)]
pub struct PostgresMemoryStore {
    pool: Pool,
    embedding_dimension: usize,
    distance_metric: PostgresDistanceMetric,
    search_surface_mode: PostgresSearchSurfaceMode,
    rerank_candidate_limit: i64,
    crypto: MemoryCryptoProvider,
}

impl std::fmt::Debug for PostgresMemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresMemoryStore")
            .field("embedding_dimension", &self.embedding_dimension)
            .field("distance_metric", &self.distance_metric)
            .field("search_surface_mode", &self.search_surface_mode)
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
            .wait_timeout(Some(config.pool_wait_timeout))
            .runtime(Runtime::Tokio1)
            .build()
            .map_err(|error| store_error("build PostgreSQL pool", error, false))?;
        let store = Self {
            pool,
            embedding_dimension: config.embedding_dimension,
            distance_metric: config.distance_metric,
            search_surface_mode: config.search_surface_mode,
            rerank_candidate_limit: config.rerank_candidate_limit,
            crypto: MemoryCryptoProvider::from_env(),
        };
        if store.search_surface_mode != PostgresSearchSurfaceMode::PlaintextPgvector {
            MemoryDecryptBrokerConfig::from_env()
                .and_then(|config| config.validate())
                .map_err(MemoryStoreError::from)?;
            if !store.crypto.is_encrypted_ready() {
                return Err(MemoryStoreError::invalid(
                    "protected PostgreSQL search mode requires a ready encrypted memory provider",
                ));
            }
        }
        store.apply_migrations().await?;
        Ok(store)
    }

    async fn client(&self) -> MemoryStoreResult<deadpool_postgres::Client> {
        self.pool
            .get()
            .await
            .map_err(|error| store_error("acquire PostgreSQL connection", error, true))
    }

    fn persisted_key_scope(
        tenant: &crate::types::MemoryTenantScope,
        org_unit: Option<String>,
        owner_subject: Option<String>,
        data_class: String,
        source_binding_id: Option<String>,
    ) -> MemoryStoreResult<MemoryKeyScope> {
        let data_class = serde_json::from_value(serde_json::Value::String(data_class))
            .map_err(|error| store_error("decode PostgreSQL memory data class", error, false))?;
        Ok(MemoryKeyScope::new(tenant, data_class, source_binding_id)
            .with_org_unit(org_unit)
            .with_owner_subject(owner_subject))
    }

    fn key_scope_columns(scope: &MemoryKeyScope) -> MemoryStoreResult<(String, Option<String>)> {
        let data_class = serde_json::to_value(scope.data_class)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .ok_or_else(|| MemoryStoreError::invalid("memory key scope has invalid data class"))?;
        Ok((data_class, scope.source_binding_id.clone()))
    }

    fn encrypt_embedding(
        &self,
        embedding: &[f32],
        key_scope: &MemoryKeyScope,
        row_id: &str,
    ) -> MemoryStoreResult<(String, Option<MemoryEnvelopeMetadata>, String, String)> {
        let policy_id = format!("memory-search-policy:{row_id}");
        let audit_id = format!("memory-search-audit:{row_id}");
        let plaintext = serde_json::to_string(embedding)
            .map_err(|error| store_error("serialize encrypted embedding", error, false))?;
        let (ciphertext, envelope) = self
            .crypto
            .encrypt_field_scoped(&plaintext, key_scope, &policy_id, &audit_id)
            .map_err(MemoryStoreError::from)?;
        Ok((ciphertext, envelope, policy_id, audit_id))
    }

    fn decrypt_embedding(
        &self,
        ciphertext: &str,
        envelope: Option<&MemoryEnvelopeMetadata>,
        key_scope: &MemoryKeyScope,
        policy_id: &str,
        audit_id: &str,
    ) -> MemoryStoreResult<Vec<f32>> {
        let principal = crate::decrypt_context::current_decrypt_principal();
        let authority = MemoryEnvelopeAuthority::new(key_scope.clone(), policy_id, audit_id);
        let plaintext = self
            .crypto
            .decrypt_field_scoped_authorized(
                ciphertext,
                envelope,
                principal.as_ref(),
                &authority,
                None,
            )
            .map_err(MemoryStoreError::from)?;
        serde_json::from_str(&plaintext)
            .map_err(|error| store_error("deserialize encrypted embedding", error, false))
    }

    fn encode_payload<T: serde::Serialize>(
        &self,
        value: &T,
        key_scope: &MemoryKeyScope,
        row_id: &str,
    ) -> MemoryStoreResult<EncodedPayload> {
        if self.crypto.is_plaintext() {
            return Ok((Some(json_value(value)?), None, None, None, None));
        }
        let policy_id = format!("memory-payload-policy:{row_id}");
        let audit_id = format!("memory-payload-audit:{row_id}");
        let plaintext = serde_json::to_string(value)
            .map_err(|error| store_error("serialize encrypted memory payload", error, false))?;
        let (ciphertext, envelope) = self
            .crypto
            .encrypt_field_scoped(&plaintext, key_scope, &policy_id, &audit_id)
            .map_err(MemoryStoreError::from)?;
        Ok((
            None,
            Some(ciphertext),
            envelope.map(|value| json_value(&value)).transpose()?,
            Some(policy_id),
            Some(audit_id),
        ))
    }

    fn decode_payload<T: serde::de::DeserializeOwned>(
        &self,
        plaintext: Option<serde_json::Value>,
        ciphertext: Option<String>,
        envelope: Option<serde_json::Value>,
        key_scope: &MemoryKeyScope,
        policy_id: Option<String>,
        audit_id: Option<String>,
    ) -> MemoryStoreResult<T> {
        if let Some(value) = plaintext {
            return from_json(value);
        }
        let ciphertext = ciphertext.ok_or_else(|| {
            MemoryStoreError::new(
                MemoryStoreErrorKind::CorruptData,
                "PostgreSQL memory row has neither plaintext nor ciphertext payload",
            )
        })?;
        let policy_id = policy_id.ok_or_else(|| {
            MemoryStoreError::new(
                MemoryStoreErrorKind::CorruptData,
                "missing payload policy id",
            )
        })?;
        let audit_id = audit_id.ok_or_else(|| {
            MemoryStoreError::new(
                MemoryStoreErrorKind::CorruptData,
                "missing payload audit id",
            )
        })?;
        let envelope = envelope.map(from_json).transpose()?;
        let principal = crate::decrypt_context::current_decrypt_principal();
        let authority = MemoryEnvelopeAuthority::new(key_scope.clone(), &policy_id, &audit_id);
        let decoded = self
            .crypto
            .decrypt_field_scoped_authorized(
                &ciphertext,
                envelope.as_ref(),
                principal.as_ref(),
                &authority,
                None,
            )
            .map_err(MemoryStoreError::from)?;
        serde_json::from_str(&decoded)
            .map_err(|error| store_error("deserialize encrypted memory payload", error, false))
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
