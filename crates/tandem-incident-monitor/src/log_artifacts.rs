// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::json;
use tokio::fs;

use crate::types::IncidentMonitorLogCandidate;

fn safe_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub async fn write_log_evidence_artifact(
    log_evidence_dir: &Path,
    candidate: &IncidentMonitorLogCandidate,
) -> anyhow::Result<String> {
    let project_id = safe_segment(&candidate.project_id);
    let source_id = safe_segment(&candidate.source_id);
    let digest = crate::sha256_hex(&[
        &candidate.project_id,
        &candidate.source_id,
        &candidate.fingerprint,
        &candidate.offset_start.to_string(),
    ]);
    let file_name = format!("{}-{}.json", &digest[..16], candidate.offset_start);
    let dir: PathBuf = log_evidence_dir.join(&project_id).join(&source_id);
    fs::create_dir_all(&dir).await?;
    let path = dir.join(&file_name);
    let payload = json!({
        "schema_version": 1,
        "project_id": candidate.project_id,
        "source_id": candidate.source_id,
        "repo": candidate.repo,
        "workspace_root": candidate.workspace_root,
        "log_path": candidate.path,
        "offset_start": candidate.offset_start,
        "offset_end": candidate.offset_end,
        "inode": candidate.inode,
        "detected_at_ms": candidate.timestamp_ms,
        "parser_format": "auto",
        "level": candidate.level,
        "event": candidate.event,
        "fingerprint": candidate.fingerprint,
        "title": candidate.title,
        "excerpt": candidate.excerpt,
        "raw_excerpt_redacted": candidate.raw_excerpt_redacted,
        "redactions": [],
    });
    let raw = serde_json::to_string_pretty(&payload)?;
    fs::write(&path, raw)
        .await
        .with_context(|| format!("failed to write log evidence artifact {}", path.display()))?;
    Ok(format!(
        "tandem://incident-monitor/{}/evidence/{}/{}",
        project_id, source_id, file_name
    ))
}
