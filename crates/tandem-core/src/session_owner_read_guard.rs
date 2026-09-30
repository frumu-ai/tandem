use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use tandem_types::{Session, TenantContext};

/// An owned, Send snapshot of requested native session owners. An IMMEDIATE
/// transaction prevents an independent connection from changing ownership
/// while the caller makes its final synchronous disclosure decision. No rows
/// are written, and the transaction is rolled back when the guard is dropped.
pub struct SessionOwnerReadGuard {
    connection: Option<Connection>,
    owners: HashMap<String, TenantContext>,
}

impl SessionOwnerReadGuard {
    pub(super) fn empty() -> Self {
        Self {
            connection: None,
            owners: HashMap::new(),
        }
    }

    pub(super) fn acquire(connection: Connection, session_ids: &[String]) -> Result<Self> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let mut guard = Self {
            connection: Some(connection),
            owners: HashMap::new(),
        };
        for session_id in session_ids {
            let raw: Option<String> = guard
                .connection
                .as_ref()
                .unwrap()
                .query_row(
                    "SELECT session_json FROM session_records WHERE session_id = ?1",
                    [session_id],
                    |row| row.get(0),
                )
                .optional()?;
            // A missing, malformed or misbound header cannot grant authority.
            // Use the native header parser's Session schema, but never query
            // message rows. Stored session_json contains only the header.
            // Expose no repository/connection handle or full session copy.
            if let Some(header) = raw
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Session>(raw).ok())
                .filter(|header| header.id == *session_id)
            {
                guard
                    .owners
                    .insert(session_id.clone(), header.tenant_context);
            }
        }
        Ok(guard)
    }

    pub fn tenant_context(&self, session_id: &str) -> Option<&TenantContext> {
        self.owners.get(session_id)
    }
}

impl Drop for SessionOwnerReadGuard {
    fn drop(&mut self) {
        if let Some(connection) = &self.connection {
            if let Err(error) = connection.execute_batch("ROLLBACK") {
                tracing::error!(%error, "failed to release session owner read transaction");
            }
        }
    }
}

#[cfg(test)]
#[path = "session_owner_read_guard_tests.rs"]
mod tests;
