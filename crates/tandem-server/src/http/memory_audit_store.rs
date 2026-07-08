use axum::http::StatusCode;
use tandem_types::TenantContext;

use crate::AppState;

pub(crate) async fn append_memory_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    mut event: crate::MemoryAuditEvent,
) -> Result<(), StatusCode> {
    event.tenant_context = tenant_context.clone();
    if let Some(parent) = state.memory_audit_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let line = serde_json::to_string(&event).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let stored_line = crate::encrypted_file_store::encrypt_jsonl_line(&line)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.memory_audit_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, stored_line.as_bytes())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, b"\n")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    file.sync_data()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut audit = state.memory_audit_log.write().await;
    audit.push(event);
    Ok(())
}

pub(crate) async fn load_memory_audit_events(
    path: &std::path::Path,
) -> Vec<crate::MemoryAuditEvent> {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Vec::new();
    };

    let mut events = Vec::new();
    for line in content.lines() {
        let plaintext = match crate::encrypted_file_store::decrypt_jsonl_line(line) {
            Ok(Some(plaintext)) => plaintext,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(
                    path = %path.display(),
                    error = ?error,
                    "failed to decrypt memory audit log"
                );
                return Vec::new();
            }
        };
        if let Ok(event) = serde_json::from_str::<crate::MemoryAuditEvent>(plaintext.trim()) {
            events.push(event);
        }
    }
    events
}
