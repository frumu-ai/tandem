# PostgreSQL memory backend

Tandem enterprise builds include the PostgreSQL + pgvector memory backend.
SQLite remains the default for local and desktop installs.

## Configuration

| Variable | Purpose |
| --- | --- |
| `TANDEM_MEMORY_BACKEND=postgres` | Select PostgreSQL instead of SQLite. |
| `TANDEM_MEMORY_POSTGRES_URL` | PostgreSQL connection URL. Required. |
| `TANDEM_MEMORY_EMBEDDING_DIMENSION` | pgvector dimension. Defaults to 384 and must match the existing schema. |
| `TANDEM_MEMORY_POSTGRES_DISTANCE` | `cosine` (default), `euclidean`, or `inner_product`. |
| `TANDEM_MEMORY_POSTGRES_POOL_SIZE` | Connection pool size, default 16. |
| `TANDEM_MEMORY_POSTGRES_POOL_WAIT_TIMEOUT_MS` | Maximum connection-pool wait, default 5000 ms (range 10-120000). Exhaustion returns a retryable unavailable error. |
| `TANDEM_MEMORY_SEARCH_SURFACE_MODE` | `plaintext_pgvector`, `encrypted_rerank`, or `disabled`. Hosted encryption defaults to `encrypted_rerank`. |
| `TANDEM_MEMORY_POSTGRES_RERANK_CANDIDATES` | Maximum scoped ciphertext candidates decrypted per query, default 1000. |

`TANDEM_MEMORY_ENCRYPTION_REQUIRED=true` fails closed if plaintext pgvector is
selected. Hosted encrypted reranking uses the existing memory KMS provider and
`TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID`.

## Schema and readiness

The backend enables the `vector` extension, applies schema changes inside a
transaction protected by a PostgreSQL advisory migration lock, and records its
version in `tandem_memory_schema_migrations`. A configured embedding dimension
that differs from the existing `vector(N)` column is rejected on startup.

`GET /global/health` exposes `memory_storage.backend`, per-check details, and
marks the process unready when the selected memory backend cannot open or fails
its health checks.

## Search guarantees

Plaintext mode uses exact pgvector ordering after tenant, deployment,
department, subject, tier, project, and session predicates. Encrypted mode stores
neither raw vectors nor searchable text. It selects a bounded recent candidate
window with the same predicates, authorizes decryption with exact envelope
authority, and reranks in process. Cross-scope rows never enter either top-k.

## Moving from SQLite

Backend selection does not copy data automatically. Before switching an
existing deployment, export memory through Tandem's governed memory export API,
configure PostgreSQL, import through the governed import API, compare tenant and
tier counts, then switch `TANDEM_MEMORY_BACKEND`. Keep the SQLite file read-only
until the PostgreSQL health and count checks pass. Never copy raw SQLite rows
directly because that bypasses scope validation and envelope re-sealing.
