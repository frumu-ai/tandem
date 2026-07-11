use super::*;

const SCHEMA_VERSION: i32 = 1;

impl PostgresMemoryStore {
    pub(super) async fn apply_migrations(&self) -> MemoryStoreResult<()> {
        let mut client = self.client().await?;
        let transaction = client
            .transaction()
            .await
            .map_err(|error| store_error("start PostgreSQL migration", error, true))?;
        transaction
            .batch_execute("CREATE EXTENSION IF NOT EXISTS vector")
            .await
            .map_err(|error| store_error("enable pgvector extension", error, false))?;
        transaction
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS tandem_memory_schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
                )",
            )
            .await
            .map_err(|error| store_error("create PostgreSQL migration ledger", error, false))?;

        let ddl = format!(
            "CREATE TABLE IF NOT EXISTS tandem_memory_chunks (
                id TEXT PRIMARY KEY,
                tenant_org_id TEXT NOT NULL,
                tenant_workspace_id TEXT NOT NULL,
                tenant_deployment_id TEXT NOT NULL DEFAULT '',
                owner_org_unit_id TEXT,
                owner_subject TEXT,
                tier TEXT NOT NULL,
                project_id TEXT,
                session_id TEXT,
                source_path TEXT,
                created_at TIMESTAMPTZ NOT NULL,
                data JSONB NOT NULL,
                embedding vector({dimension}) NOT NULL
            );
            CREATE INDEX IF NOT EXISTS tandem_memory_chunks_scope_idx ON tandem_memory_chunks
                (tenant_org_id, tenant_workspace_id, tenant_deployment_id, tier,
                 project_id, session_id, owner_org_unit_id, owner_subject);
            CREATE TABLE IF NOT EXISTS tandem_memory_global_records (
                id TEXT PRIMARY KEY,
                tenant_org_id TEXT NOT NULL,
                tenant_workspace_id TEXT NOT NULL,
                tenant_deployment_id TEXT NOT NULL DEFAULT '',
                owner_org_unit_id TEXT,
                owner_subject TEXT,
                private BOOLEAN NOT NULL,
                user_id TEXT NOT NULL,
                source_type TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                run_id TEXT NOT NULL,
                session_id TEXT,
                message_id TEXT,
                tool_name TEXT,
                project_tag TEXT,
                channel_tag TEXT,
                demoted BOOLEAN NOT NULL,
                expires_at_ms BIGINT,
                created_at_ms BIGINT NOT NULL,
                search_content TEXT NOT NULL,
                data JSONB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS tandem_memory_global_scope_idx ON tandem_memory_global_records
                (tenant_org_id, tenant_workspace_id, tenant_deployment_id,
                 owner_org_unit_id, owner_subject, private, user_id, created_at_ms DESC);
            CREATE INDEX IF NOT EXISTS tandem_memory_global_fts_idx ON tandem_memory_global_records
                USING GIN (to_tsvector('simple', search_content));
            CREATE UNIQUE INDEX IF NOT EXISTS tandem_memory_global_dedupe_idx
                ON tandem_memory_global_records (
                    tenant_org_id, tenant_workspace_id, tenant_deployment_id, user_id,
                    source_type, content_hash, run_id, COALESCE(session_id, ''),
                    COALESCE(message_id, ''), COALESCE(tool_name, ''),
                    COALESCE(owner_org_unit_id, ''), private, COALESCE(owner_subject, ''));
            CREATE TABLE IF NOT EXISTS tandem_memory_entities (
                tenant_org_id TEXT NOT NULL,
                tenant_workspace_id TEXT NOT NULL,
                tenant_deployment_id TEXT NOT NULL DEFAULT '',
                entity_type TEXT NOT NULL,
                key1 TEXT NOT NULL,
                key2 TEXT NOT NULL DEFAULT '',
                data JSONB NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (tenant_org_id, tenant_workspace_id,
                    tenant_deployment_id, entity_type, key1, key2)
            );
            CREATE INDEX IF NOT EXISTS tandem_memory_entities_lookup_idx ON tandem_memory_entities
                (tenant_org_id, tenant_workspace_id, tenant_deployment_id, entity_type, key1, key2);",
            dimension = self.embedding_dimension
        );
        transaction
            .batch_execute(&ddl)
            .await
            .map_err(|error| store_error("apply PostgreSQL memory schema", error, false))?;
        transaction
            .execute(
                "INSERT INTO tandem_memory_schema_migrations(version, name)
                 VALUES ($1, 'postgres_memory_store_v1') ON CONFLICT (version) DO NOTHING",
                &[&SCHEMA_VERSION],
            )
            .await
            .map_err(|error| store_error("record PostgreSQL memory migration", error, false))?;
        transaction
            .commit()
            .await
            .map_err(|error| store_error("commit PostgreSQL migration", error, true))?;
        Ok(())
    }
}
