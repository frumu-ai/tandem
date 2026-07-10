impl MemoryDatabase {
    fn ensure_chunk_scope_columns(
        &self,
        conn: &Connection,
        table: &str,
        existing_cols: &HashSet<String>,
    ) -> MemoryResult<()> {
        if !existing_cols.contains("owner_org_unit_id") {
            conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN owner_org_unit_id TEXT"),
                [],
            )?;
        }
        if !existing_cols.contains("tenant_shared") {
            conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN tenant_shared INTEGER NOT NULL DEFAULT 0"),
                [],
            )?;
        }
        // Per-scope envelope for hosted-KMS encryption (TAN-668). NULL for
        // local/plaintext rows; carries the wrapped DEK + key scope for hosted
        // rows so `content`/`metadata` can be decrypted before use.
        if !existing_cols.contains("crypto_envelope") {
            conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN crypto_envelope TEXT"),
                [],
            )?;
        }
        self.backfill_chunk_scope_columns(conn, table)
    }

    fn backfill_chunk_scope_columns(&self, conn: &Connection, table: &str) -> MemoryResult<()> {
        let rows = {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, metadata FROM {table}
                 WHERE (owner_org_unit_id IS NULL OR tenant_shared = 0)
                   AND metadata IS NOT NULL
                   AND TRIM(metadata) != ''"
            ))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        for (id, metadata_stored) in rows {
            let Some(metadata_stored) = metadata_stored else {
                continue;
            };
            let metadata_plain = match self.crypto.decrypt_field(&metadata_stored) {
                Ok(metadata_plain) => metadata_plain,
                Err(err) => {
                    tracing::warn!(
                        table = table,
                        chunk_id = id.as_str(),
                        "skipping owner_org_unit_id backfill for unreadable chunk metadata: {}",
                        err
                    );
                    continue;
                }
            };
            let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&metadata_plain) else {
                continue;
            };
            let owner_org_unit_id = owner_org_unit_id_from_metadata(Some(&metadata));
            let tenant_shared = tenant_shared_from_metadata(Some(&metadata));
            if owner_org_unit_id.is_none() && !tenant_shared {
                continue;
            }
            conn.execute(
                &format!(
                    "UPDATE {table}
                     SET owner_org_unit_id = COALESCE(owner_org_unit_id, ?1),
                         tenant_shared = CASE WHEN ?2 = 1 THEN 1 ELSE tenant_shared END
                     WHERE id = ?3"
                ),
                params![owner_org_unit_id.as_deref(), i64::from(tenant_shared), id],
            )?;
        }

        Ok(())
    }
}
