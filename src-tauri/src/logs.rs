use crate::error::{Result, TandemError};
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct RingLine {
    pub seq: u64,
    pub text: String,
}

#[derive(Debug)]
pub struct LogRingBuffer {
    max_lines: usize,
    seq: AtomicU64,
    dropped_total: AtomicU64,
    lines: RwLock<VecDeque<RingLine>>,
}

impl LogRingBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            max_lines,
            seq: AtomicU64::new(0),
            dropped_total: AtomicU64::new(0),
            lines: RwLock::new(VecDeque::new()),
        }
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped_total.load(Ordering::Relaxed)
    }

    pub fn push(&self, text: String) -> u64 {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let mut guard = self.lines.write().unwrap();
        guard.push_back(RingLine { seq, text });

        while guard.len() > self.max_lines {
            guard.pop_front();
            self.dropped_total.fetch_add(1, Ordering::Relaxed);
        }

        seq
    }

    pub fn snapshot(&self, last_n: usize) -> Vec<RingLine> {
        let guard = self.lines.read().unwrap();
        let len = guard.len();
        let start = len.saturating_sub(last_n);
        guard.iter().skip(start).cloned().collect()
    }

    pub fn since(&self, seq: u64) -> Vec<RingLine> {
        let guard = self.lines.read().unwrap();
        guard.iter().filter(|l| l.seq > seq).cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogFileInfo {
    pub name: String,
    pub size: u64,
    pub modified_ms: u64,
}

pub fn list_log_files(logs_dir: &Path) -> Result<Vec<LogFileInfo>> {
    let mut out = Vec::new();
    let entries = fs::read_dir(logs_dir).map_err(TandemError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // We only expose Tandem's own rolling log files. (e.g. tandem.YYYY-MM-DD.log)
        if !file_name.starts_with("tandem") {
            continue;
        }

        let meta = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.push(LogFileInfo {
            name: file_name,
            size: meta.len(),
            modified_ms,
        });
    }

    out.sort_by_key(|f| std::cmp::Reverse(f.modified_ms));
    Ok(out)
}

// Best-effort "tail" by bytes and split into lines. Returns (lines, start_offset).
pub fn tail_file(path: &Path, tail_lines: usize, max_bytes: u64) -> Result<(Vec<String>, u64)> {
    let mut file = File::open(path).map_err(TandemError::Io)?;
    let meta = file.metadata().map_err(TandemError::Io)?;
    let len = meta.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start)).map_err(TandemError::Io)?;

    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(TandemError::Io)?;
    let s = String::from_utf8_lossy(&buf);

    let mut lines: Vec<String> = s
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.len() > tail_lines {
        lines = lines.split_off(lines.len() - tail_lines);
    }

    Ok((lines, len))
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

pub fn sanitize_log_file_name(name: &str) -> Result<String> {
    // Avoid path traversal: only allow a simple file name.
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(TandemError::InvalidConfig(
            "Invalid log file name".to_string(),
        ));
    }
    Ok(name.to_string())
}

pub fn join_logs_dir(logs_dir: &Path, file_name: &str) -> PathBuf {
    logs_dir.join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_drops_oldest() {
        let buf = LogRingBuffer::new(3);
        buf.push("a".to_string());
        buf.push("b".to_string());
        buf.push("c".to_string());
        buf.push("d".to_string());
        let snap = buf.snapshot(10);
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].text, "b");
        assert_eq!(buf.dropped_total(), 1);
    }

    #[test]
    fn ring_buffer_since_seq() {
        let buf = LogRingBuffer::new(10);
        let s1 = buf.push("one".to_string());
        let s2 = buf.push("two".to_string());
        let s3 = buf.push("three".to_string());
        let since = buf.since(s1);
        assert_eq!(since.len(), 2);
        assert_eq!(since[0].seq, s2);
        assert_eq!(since[1].seq, s3);
    }
}
