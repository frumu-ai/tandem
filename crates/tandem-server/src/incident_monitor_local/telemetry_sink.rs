// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::{sha256_hex, AppState, Value};

const MAX_TELEMETRY_LINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_LEGACY_RECEIPT_RECOVERY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(serde::Deserialize)]
struct TelemetryReceiptIdentity {
    provider: Option<String>,
    operation: Option<String>,
    status: Option<String>,
    record_id: Option<String>,
    idempotency_key: Option<String>,
    destination_id: Option<String>,
    target_ref: Option<String>,
}

impl TelemetryReceiptIdentity {
    fn lookup_key(&self) -> Option<String> {
        telemetry_lookup_key_fields(
            self.provider.as_deref(),
            self.operation.as_deref(),
            self.status.as_deref(),
            self.record_id.as_deref(),
            self.idempotency_key.as_deref(),
            self.destination_id.as_deref(),
            self.target_ref.as_deref(),
        )
    }
}

#[derive(Default)]
struct TelemetrySinkIndex {
    scanned_len: u64,
    receipt_offsets: std::collections::HashMap<String, (u64, u64)>,
    #[cfg(unix)]
    file_identity: Option<(u64, u64)>,
    #[cfg(unix)]
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    changed_at: Option<(i64, i64)>,
}

impl TelemetrySinkIndex {
    fn prepare_file(&mut self, metadata: &std::fs::Metadata) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let identity = (metadata.dev(), metadata.ino());
            let modified = metadata.modified().ok();
            let changed_at = (metadata.ctime(), metadata.ctime_nsec());
            if self.file_identity != Some(identity)
                || metadata.len() < self.scanned_len
                || (self.modified.is_some() && self.modified != modified)
                || (self.changed_at.is_some() && self.changed_at != Some(changed_at))
            {
                self.scanned_len = 0;
                self.receipt_offsets.clear();
            }
            self.file_identity = Some(identity);
        }
        #[cfg(not(unix))]
        {
            // Without a portable file identity, rescan rather than risk
            // trusting offsets from a replaced file.
            let _ = metadata;
            self.scanned_len = 0;
            self.receipt_offsets.clear();
        }
    }

    fn mark_synced(&mut self, metadata: &std::fs::Metadata) {
        self.scanned_len = metadata.len();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            self.modified = metadata.modified().ok();
            self.changed_at = Some((metadata.ctime(), metadata.ctime_nsec()));
        }
    }
}

fn telemetry_sink_index(
    path: &std::path::Path,
) -> anyhow::Result<std::sync::Arc<std::sync::Mutex<TelemetrySinkIndex>>> {
    use std::sync::{Arc, Mutex, OnceLock};

    static INDEXES: OnceLock<
        Mutex<std::collections::HashMap<std::path::PathBuf, Arc<Mutex<TelemetrySinkIndex>>>>,
    > = OnceLock::new();
    let mut indexes = INDEXES
        .get_or_init(|| Mutex::new(std::collections::HashMap::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("telemetry sink index registry lock poisoned"))?;
    if !indexes.contains_key(path) && indexes.len() >= 128 {
        // This is a performance cache, not the source of truth. Eviction only
        // forces a fresh disk scan for the next use of an old sink.
        indexes.clear();
    }
    Ok(indexes
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(TelemetrySinkIndex::default())))
        .clone())
}

fn telemetry_receipt_lookup_key(value: &Value) -> Option<String> {
    telemetry_lookup_key_fields(
        value.get("provider").and_then(Value::as_str),
        value.get("operation").and_then(Value::as_str),
        value.get("status").and_then(Value::as_str),
        value.get("record_id").and_then(Value::as_str),
        value.get("idempotency_key").and_then(Value::as_str),
        value.get("destination_id").and_then(Value::as_str),
        value.get("target_ref").and_then(Value::as_str),
    )
}

fn telemetry_lookup_key_fields(
    provider: Option<&str>,
    operation: Option<&str>,
    status: Option<&str>,
    record_id: Option<&str>,
    idempotency_key: Option<&str>,
    destination_id: Option<&str>,
    target_ref: Option<&str>,
) -> Option<String> {
    if provider? != "incident_monitor_telemetry"
        || operation? != "record_telemetry"
        || status? != "posted"
    {
        return None;
    }
    Some(sha256_hex(&[
        record_id?,
        idempotency_key?,
        destination_id?,
        target_ref?,
    ]))
}

struct TelemetryLineRead {
    length: u64,
    terminated: bool,
    oversized: bool,
}

fn read_telemetry_line(
    reader: &mut impl std::io::BufRead,
    line: &mut Vec<u8>,
) -> anyhow::Result<TelemetryLineRead> {
    use anyhow::Context;
    line.clear();
    let mut length = 0_u64;
    let mut oversized = false;
    loop {
        let available = reader.fill_buf().context("read telemetry sink")?;
        if available.is_empty() {
            return Ok(TelemetryLineRead {
                length,
                terminated: false,
                oversized,
            });
        }
        let bytes = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        length = length
            .checked_add(bytes as u64)
            .ok_or_else(|| anyhow::anyhow!("telemetry sink line length overflow"))?;
        if !oversized {
            if length <= MAX_TELEMETRY_LINE_BYTES as u64 {
                line.extend_from_slice(&available[..bytes]);
            } else {
                // Historical sinks could contain arbitrarily large records.
                // Stream past them, then parse only their identity from disk.
                oversized = true;
                line.clear();
            }
        }
        let terminated = available[bytes - 1] == b'\n';
        reader.consume(bytes);
        if terminated {
            return Ok(TelemetryLineRead {
                length,
                terminated: true,
                oversized,
            });
        }
    }
}

fn scan_telemetry_sink(
    file: &mut std::fs::File,
    index: &mut TelemetrySinkIndex,
) -> anyhow::Result<bool> {
    use std::io::{BufReader, Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(index.scanned_len))?;
    let mut line = Vec::new();
    let mut cursor = index.scanned_len;
    let mut added = Vec::new();
    let mut large_lines = Vec::new();
    let mut unterminated_tail = false;
    let mut tail_start = cursor;
    {
        let mut reader = BufReader::new(&mut *file);
        loop {
            let current = read_telemetry_line(&mut reader, &mut line)?;
            if current.length == 0 {
                break;
            }
            tail_start = cursor;
            if current.oversized {
                large_lines.push((cursor, current.length));
            } else if let Ok(candidate) = serde_json::from_slice::<Value>(&line) {
                if let Some(key) = telemetry_receipt_lookup_key(&candidate) {
                    added.push((key, cursor, current.length));
                }
            }
            cursor += current.length;
            unterminated_tail = !current.terminated;
        }
    }
    for (offset, length) in large_lines {
        if length > MAX_LEGACY_RECEIPT_RECOVERY_BYTES {
            anyhow::bail!(
                "legacy telemetry sink line exceeds the {MAX_LEGACY_RECEIPT_RECOVERY_BYTES}-byte safety limit"
            );
        }
        file.seek(SeekFrom::Start(offset))?;
        let bounded = (&mut *file).take(length);
        match serde_json::from_reader::<_, TelemetryReceiptIdentity>(bounded) {
            Ok(identity) => {
                if let Some(key) = identity.lookup_key() {
                    added.push((key, offset, length));
                }
            }
            Err(error) if error.is_io() => return Err(error.into()),
            Err(_) => {
                // Malformed legacy lines cannot prove delivery and are left
                // intact; later valid records remain publishable.
            }
        }
    }
    for (key, offset, length) in added {
        index.receipt_offsets.entry(key).or_insert((offset, length));
    }
    // If a write is rejected or interrupted before we repair the delimiter,
    // the next attempt must reread this tail rather than appending into it.
    index.scanned_len = if unterminated_tail {
        tail_start
    } else {
        cursor
    };
    Ok(unterminated_tail)
}

fn sync_telemetry_sink_directories(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use anyhow::Context;
        // The post ledger may be fsynced in a different directory. Make the
        // sink filename and any newly created ancestor directory names durable
        // before allowing that ledger to say `posted`.
        if let Some(parent) = path.parent() {
            for directory in parent.ancestors() {
                if directory.as_os_str().is_empty() {
                    continue;
                }
                std::fs::File::open(directory)
                    .and_then(|file| file.sync_all())
                    .with_context(|| format!("sync telemetry directory {}", directory.display()))?;
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn ensure_telemetry_sink_path_matches_file(
    path: &std::path::Path,
    file: &std::fs::File,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata()?;
        let current = std::fs::metadata(path)?;
        if (opened.dev(), opened.ino()) != (current.dev(), current.ino()) {
            anyhow::bail!(
                "telemetry sink path changed during publication: {}",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, file);
    }
    Ok(())
}

/// Reconcile a telemetry receipt with its JSONL sink before appending. The sink
/// and post ledger are separate durable files, so a crash after the sink flush
/// but before the post update must recover the sink receipt on retry.
pub(super) async fn persist_incident_monitor_telemetry(
    state: &AppState,
    path: &std::path::Path,
    receipt: &Value,
) -> anyhow::Result<Value> {
    use anyhow::Context;
    crate::incident_monitor::require_current_policy(state)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create telemetry sink directory {}", parent.display()))?;
        }
    }
    crate::incident_monitor::require_current_policy(state)?;
    let state = state.clone();
    let path = path.to_path_buf();
    let receipt = receipt.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek, SeekFrom, Write};

        crate::incident_monitor::require_current_policy(&state)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open telemetry sink {}", path.display()))?;
        // Claim expiry permits another publisher after ten minutes. Hold the
        // file lock across both the scan and append so even overlapping workers
        // cannot write the same deterministic record twice.
        fs2::FileExt::lock_exclusive(&file)
            .with_context(|| format!("lock telemetry sink {}", path.display()))?;
        ensure_telemetry_sink_path_matches_file(&path, &file)?;
        crate::incident_monitor::require_current_policy(&state)?;
        let index_handle = telemetry_sink_index(&path)?;
        let mut index = index_handle
            .lock()
            .map_err(|_| anyhow::anyhow!("telemetry sink index lock poisoned"))?;
        index.prepare_file(&file.metadata()?);
        let unterminated_tail = scan_telemetry_sink(&mut file, &mut index)?;
        let lookup_key = telemetry_receipt_lookup_key(&receipt);
        let existing_location = lookup_key
            .as_ref()
            .and_then(|key| index.receipt_offsets.get(key))
            .copied();
        if let Some((offset, length)) = existing_location {
            if length > MAX_LEGACY_RECEIPT_RECOVERY_BYTES {
                anyhow::bail!(
                    "matching legacy telemetry receipt exceeds {MAX_LEGACY_RECEIPT_RECOVERY_BYTES} bytes"
                );
            }
            file.seek(SeekFrom::Start(offset))?;
            let existing: Value = serde_json::from_reader((&mut file).take(length))?;
            if !same_telemetry_receipt(&existing, &receipt) {
                anyhow::bail!("telemetry sink receipt index does not match the expected record");
            }
            // A complete final JSON object can exist without its newline if a
            // write was interrupted. Finish the delimiter before future appends.
            if unterminated_tail {
                crate::incident_monitor::require_current_policy(&state)?;
                file.write_all(b"\n")?;
            }
            // A prior worker may have flushed a visible line but crashed
            // before syncing it. Make delivery durable before the post ledger
            // is allowed to say `posted`.
            file.sync_data()?;
            sync_telemetry_sink_directories(&path)?;
            ensure_telemetry_sink_path_matches_file(&path, &file)?;
            index.mark_synced(&file.metadata()?);
            return Ok(existing);
        }

        crate::incident_monitor::require_current_policy(&state)?;
        // Preserve a malformed or incomplete final line, but never splice a
        // valid new receipt onto it.
        if unterminated_tail {
            file.write_all(b"\n")?;
        }
        let append_offset = file.metadata()?.len();
        let mut line = serde_json::to_vec(&receipt)?;
        if line.len().saturating_add(1) > MAX_TELEMETRY_LINE_BYTES {
            anyhow::bail!("telemetry receipt exceeds {MAX_TELEMETRY_LINE_BYTES} bytes");
        }
        line.push(b'\n');
        file.write_all(&line)
            .with_context(|| format!("append telemetry record to {}", path.display()))?;
        file.flush()
            .with_context(|| format!("flush telemetry record to {}", path.display()))?;
        file.sync_data()
            .with_context(|| format!("sync telemetry record to {}", path.display()))?;
        sync_telemetry_sink_directories(&path)?;
        ensure_telemetry_sink_path_matches_file(&path, &file)?;
        if let Some(key) = lookup_key {
            index
                .receipt_offsets
                .insert(key, (append_offset, line.len() as u64));
        }
        index.mark_synced(&file.metadata()?);
        Ok(receipt)
    })
    .await
    .context("telemetry sink worker failed")?
}

fn same_telemetry_receipt(candidate: &Value, expected: &Value) -> bool {
    candidate.get("provider").and_then(Value::as_str) == Some("incident_monitor_telemetry")
        && candidate.get("operation").and_then(Value::as_str) == Some("record_telemetry")
        && candidate.get("status").and_then(Value::as_str) == Some("posted")
        && [
            "record_id",
            "idempotency_key",
            "destination_id",
            "target_ref",
        ]
        .iter()
        .all(|key| {
            let expected = expected.get(*key).and_then(Value::as_str);
            expected.is_some() && candidate.get(*key).and_then(Value::as_str) == expected
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_legacy_line_allows_new_records_and_exact_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::app::state::tests::test_state_with_path(temp.path().join("state.json"));
        let sink = temp.path().join("events.jsonl");
        let legacy = serde_json::json!({
            "provider": "incident_monitor_telemetry",
            "operation": "record_telemetry",
            "status": "posted",
            "record_id": "bmtel_legacy-large",
            "idempotency_key": "legacy-large",
            "destination_id": "telemetry-primary",
            "target_ref": "telemetry:events",
            "padding": "x".repeat(MAX_TELEMETRY_LINE_BYTES),
        });
        tokio::fs::write(&sink, format!("{legacy}\n"))
            .await
            .unwrap();
        let mut fresh = legacy.clone();
        fresh.as_object_mut().unwrap().remove("padding");
        fresh["record_id"] = serde_json::json!("bmtel_fresh");
        fresh["idempotency_key"] = serde_json::json!("fresh");

        assert_eq!(
            persist_incident_monitor_telemetry(&state, &sink, &fresh)
                .await
                .unwrap(),
            fresh
        );
        assert_eq!(
            persist_incident_monitor_telemetry(&state, &sink, &legacy)
                .await
                .unwrap(),
            legacy
        );
        assert_eq!(
            tokio::fs::read_to_string(&sink)
                .await
                .unwrap()
                .lines()
                .count(),
            2
        );
    }

    #[test]
    fn unterminated_tail_keeps_checkpoint_for_retry() {
        let temp = tempfile::tempdir().unwrap();
        let sink = temp.path().join("events.jsonl");
        std::fs::write(&sink, b"{incomplete").unwrap();
        let mut file = std::fs::OpenOptions::new().read(true).open(&sink).unwrap();
        let mut index = TelemetrySinkIndex::default();
        assert!(scan_telemetry_sink(&mut file, &mut index).unwrap());
        assert_eq!(index.scanned_len, 0);
    }
}
