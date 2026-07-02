use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

pub async fn write_file_atomically(
    path: &Path,
    content: &[u8],
    store_label: &str,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!(
                "failed to create {store_label} directory {}",
                parent.display()
            )
        })?;
    }

    let tmp_path = tmp_path_for(path);
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .with_context(|| format!("failed to create {store_label} {}", tmp_path.display()))?;
    file.write_all(content)
        .await
        .with_context(|| format!("failed to write {store_label} {}", tmp_path.display()))?;
    file.flush()
        .await
        .with_context(|| format!("failed to flush {store_label} {}", tmp_path.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("failed to sync {store_label} {}", tmp_path.display()))?;
    drop(file);

    tokio::fs::rename(&tmp_path, path)
        .await
        .with_context(|| format!("failed to publish {store_label} {}", path.display()))?;
    sync_parent_dir(path, store_label).await?;
    Ok(())
}

pub async fn repair_jsonl_torn_tail(path: &Path, store_label: &str) -> anyhow::Result<()> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {store_label} {}", path.display()))
        }
    };
    if metadata.len() == 0 {
        return Ok(());
    }

    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .await
        .with_context(|| format!("failed to open {store_label} {}", path.display()))?;

    file.seek(SeekFrom::End(-1))
        .await
        .with_context(|| format!("failed to seek {store_label} {}", path.display()))?;
    let mut last_byte = [0_u8; 1];
    file.read_exact(&mut last_byte)
        .await
        .with_context(|| format!("failed to read {store_label} {}", path.display()))?;
    if last_byte[0] == b'\n' {
        return Ok(());
    }

    file.seek(SeekFrom::Start(0))
        .await
        .with_context(|| format!("failed to rewind {store_label} {}", path.display()))?;
    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut content)
        .await
        .with_context(|| format!("failed to read {store_label} {}", path.display()))?;
    let repaired_len = content
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let tail = &content[repaired_len..];
    if serde_json::from_slice::<Value>(tail).is_ok() {
        tracing::warn!(
            path = %path.display(),
            store = store_label,
            "repairing JSONL tail by appending missing newline"
        );
        file.seek(SeekFrom::End(0))
            .await
            .with_context(|| format!("failed to seek {store_label} {}", path.display()))?;
        file.write_all(b"\n").await.with_context(|| {
            format!(
                "failed to append missing newline to {store_label} {}",
                path.display()
            )
        })?;
        file.flush()
            .await
            .with_context(|| format!("failed to flush {store_label} {}", path.display()))?;
        file.sync_all()
            .await
            .with_context(|| format!("failed to sync repaired {store_label} {}", path.display()))?;
        drop(file);
        sync_parent_dir(path, store_label).await?;
        return Ok(());
    }

    let truncated_bytes = content.len().saturating_sub(repaired_len);

    tracing::warn!(
        path = %path.display(),
        store = store_label,
        truncated_bytes,
        "truncating torn JSONL tail before append"
    );
    file.set_len(repaired_len as u64)
        .await
        .with_context(|| format!("failed to truncate {store_label} {}", path.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("failed to sync repaired {store_label} {}", path.display()))?;
    drop(file);
    sync_parent_dir(path, store_label).await?;
    Ok(())
}

pub fn sideline_corrupt_state_file_sync(
    path: &Path,
    store_label: &str,
    parse_error: impl std::fmt::Display,
) -> anyhow::Error {
    let parse_error = parse_error.to_string();
    let corrupt_path = next_corrupt_path_for(path);
    match std::fs::rename(path, &corrupt_path) {
        Ok(()) => {
            tracing::warn!(
                path = %path.display(),
                corrupt_path = %corrupt_path.display(),
                store = store_label,
                error = %parse_error,
                "sidelined corrupt state store"
            );
            anyhow::anyhow!(
                "failed to parse {store_label} {}; corrupt store moved to {}: {parse_error}",
                path.display(),
                corrupt_path.display()
            )
        }
        Err(sideline_error) => anyhow::anyhow!(
            "failed to parse {store_label} {}; also failed to sideline corrupt store: {parse_error}; {sideline_error}",
            path.display()
        ),
    }
}

#[cfg(unix)]
pub async fn sync_parent_dir(path: &Path, store_label: &str) -> anyhow::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let parent = parent.to_path_buf();
    let parent_for_sync = parent.clone();
    tokio::task::spawn_blocking(move || {
        std::fs::File::open(&parent_for_sync).and_then(|file| file.sync_all())
    })
    .await
    .with_context(|| format!("failed to join {store_label} directory sync task"))?
    .with_context(|| {
        format!(
            "failed to sync {store_label} directory {}",
            parent.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
pub async fn sync_parent_dir(_path: &Path, _store_label: &str) -> anyhow::Result<()> {
    Ok(())
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());
    tmp.set_extension(extension);
    tmp
}

fn next_corrupt_path_for(path: &Path) -> PathBuf {
    for attempt in 0..1000 {
        let candidate = corrupt_path_for_attempt(path, attempt);
        if !candidate.exists() {
            return candidate;
        }
    }
    corrupt_path_for_attempt(path, 1000)
}

fn corrupt_path_for_attempt(path: &Path, attempt: usize) -> PathBuf {
    let mut corrupt = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!("{value}.corrupt"))
        .unwrap_or_else(|| "corrupt".to_string());
    let extension = if attempt == 0 {
        extension
    } else {
        format!("{extension}.{attempt}")
    };
    corrupt.set_extension(extension);
    corrupt
}
