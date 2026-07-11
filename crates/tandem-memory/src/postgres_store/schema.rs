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
            .execute(
                "SELECT pg_advisory_xact_lock(hashtext('tandem_memory_schema'))",
                &[],
            )
            .await
            .map_err(|error| store_error("lock PostgreSQL memory migrations", error, true))?;
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
                tenant_shared BOOLEAN NOT NULL DEFAULT false,
                data_class TEXT NOT NULL DEFAULT 'internal',
                source_binding_id TEXT,
                source TEXT NOT NULL DEFAULT '',
                tier TEXT NOT NULL,
                project_id TEXT,
                session_id TEXT,
                source_path TEXT,
                created_at TIMESTAMPTZ NOT NULL,
                data JSONB,
                data_ciphertext TEXT,
                data_envelope JSONB,
                data_policy_decision_id TEXT,
                data_audit_id TEXT,
                embedding vector({dimension}),
                embedding_ciphertext TEXT,
                embedding_envelope JSONB,
                search_policy_decision_id TEXT,
                search_audit_id TEXT,
                CONSTRAINT tandem_memory_chunks_one_embedding
                  CHECK ((embedding IS NOT NULL)::int + (embedding_ciphertext IS NOT NULL)::int <= 1)
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
                data_class TEXT NOT NULL DEFAULT 'internal',
                source_binding_id TEXT,
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
                data JSONB,
                data_ciphertext TEXT,
                data_envelope JSONB,
                data_policy_decision_id TEXT,
                data_audit_id TEXT
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
            .batch_execute(
                "ALTER TABLE tandem_memory_chunks ALTER COLUMN embedding DROP NOT NULL;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS embedding_ciphertext TEXT;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS embedding_envelope JSONB;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS search_policy_decision_id TEXT;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS search_audit_id TEXT;
                 ALTER TABLE tandem_memory_chunks ALTER COLUMN data DROP NOT NULL;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS data_ciphertext TEXT;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS data_envelope JSONB;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS data_policy_decision_id TEXT;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS data_audit_id TEXT;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS tenant_shared BOOLEAN NOT NULL DEFAULT false;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS data_class TEXT NOT NULL DEFAULT 'internal';
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS source_binding_id TEXT;
                 ALTER TABLE tandem_memory_chunks ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT '';
                 UPDATE tandem_memory_chunks SET source='file' WHERE source='' AND source_path IS NOT NULL;
                 ALTER TABLE tandem_memory_global_records ALTER COLUMN data DROP NOT NULL;
                 ALTER TABLE tandem_memory_global_records ADD COLUMN IF NOT EXISTS data_ciphertext TEXT;
                 ALTER TABLE tandem_memory_global_records ADD COLUMN IF NOT EXISTS data_envelope JSONB;
                 ALTER TABLE tandem_memory_global_records ADD COLUMN IF NOT EXISTS data_policy_decision_id TEXT;
                 ALTER TABLE tandem_memory_global_records ADD COLUMN IF NOT EXISTS data_audit_id TEXT;
                 ALTER TABLE tandem_memory_global_records ADD COLUMN IF NOT EXISTS data_class TEXT NOT NULL DEFAULT 'internal';
                 ALTER TABLE tandem_memory_global_records ADD COLUMN IF NOT EXISTS source_binding_id TEXT;
                 DO $$ BEGIN
                   IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname='tandem_memory_chunks_one_embedding') THEN
                     ALTER TABLE tandem_memory_chunks ADD CONSTRAINT tandem_memory_chunks_one_embedding
                       CHECK ((embedding IS NOT NULL)::int + (embedding_ciphertext IS NOT NULL)::int <= 1);
                   END IF;
                 END $$;",
            )
            .await
            .map_err(|error| store_error("upgrade PostgreSQL search surface", error, false))?;
        transaction
            .execute(
                "INSERT INTO tandem_memory_schema_migrations(version, name)
                 VALUES ($1, 'postgres_memory_store_v1') ON CONFLICT (version) DO NOTHING",
                &[&SCHEMA_VERSION],
            )
            .await
            .map_err(|error| store_error("record PostgreSQL memory migration", error, false))?;
        let vector_type: String = transaction
            .query_one(
                "SELECT format_type(atttypid, atttypmod) FROM pg_attribute
                 WHERE attrelid='tandem_memory_chunks'::regclass AND attname='embedding'",
                &[],
            )
            .await
            .map_err(|error| store_error("inspect PostgreSQL embedding dimension", error, false))?
            .get(0);
        let expected = format!("vector({})", self.embedding_dimension);
        if vector_type != expected {
            return Err(MemoryStoreError::invalid(format!(
                "PostgreSQL embedding dimension mismatch: schema is {vector_type}, configured {expected}"
            )));
        }
        transaction
            .commit()
            .await
            .map_err(|error| store_error("commit PostgreSQL migration", error, true))?;
        Ok(())
    }
}
