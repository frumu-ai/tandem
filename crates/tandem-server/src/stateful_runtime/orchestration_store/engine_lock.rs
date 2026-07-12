use std::{
    fs::{File, OpenOptions},
    io::{Seek as _, SeekFrom, Write as _},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProcessIdentity {
    Alive(String),
    Dead,
    Unknown,
}

/// Identity of the engine process holding (or last holding) the runtime lock.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineLockOwner {
    pub pid: u32,
    pub acquired_at_ms: u64,
    #[serde(default)]
    pub process_start_hint: Option<String>,
}

impl EngineLockOwner {
    fn current(acquired_at_ms: u64) -> Self {
        Self::for_pid(std::process::id(), acquired_at_ms)
    }

    pub(super) fn for_pid(pid: u32, acquired_at_ms: u64) -> Self {
        let process_start_hint = match process_identity(pid) {
            ProcessIdentity::Alive(identity) => Some(identity),
            ProcessIdentity::Dead | ProcessIdentity::Unknown => None,
        };
        Self {
            pid,
            acquired_at_ms,
            process_start_hint,
        }
    }

    /// Returns false only when the process is absent or the PID belongs to a
    /// different process instance than the one that wrote the lock.
    pub fn is_alive(&self) -> Option<bool> {
        match process_identity(self.pid) {
            ProcessIdentity::Dead => Some(false),
            ProcessIdentity::Unknown => None,
            ProcessIdentity::Alive(identity) => Some(
                self.process_start_hint
                    .as_ref()
                    .filter(|hint| is_identity_hint(hint))
                    .is_none_or(|hint| hint == &identity),
            ),
        }
    }
}

fn is_identity_hint(value: &str) -> bool {
    value.starts_with("linux-start:")
        || value.starts_with("unix-start:")
        || value.starts_with("windows-start:")
}

#[cfg(target_os = "linux")]
fn process_identity(pid: u32) -> ProcessIdentity {
    let path = format!("/proc/{pid}/stat");
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ProcessIdentity::Dead,
        Err(_) => return ProcessIdentity::Unknown,
    };
    let Some(after_name) = raw.rsplit_once(')').map(|(_, suffix)| suffix) else {
        return ProcessIdentity::Unknown;
    };
    let Some(start_ticks) = after_name.split_whitespace().nth(19) else {
        return ProcessIdentity::Unknown;
    };
    ProcessIdentity::Alive(format!("linux-start:{start_ticks}"))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_identity(pid: u32) -> ProcessIdentity {
    let output = match std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
    {
        Ok(output) => output,
        Err(_) => return ProcessIdentity::Unknown,
    };
    if !output.status.success() {
        return ProcessIdentity::Dead;
    }
    let started = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if started.is_empty() {
        ProcessIdentity::Dead
    } else {
        ProcessIdentity::Alive(format!("unix-start:{started}"))
    }
}

#[cfg(windows)]
fn process_identity(pid: u32) -> ProcessIdentity {
    let script = format!(
        "$process = Get-Process -Id {pid} -ErrorAction SilentlyContinue; if ($null -eq $process) {{ exit 3 }}; $process.StartTime.ToUniversalTime().Ticks; exit 0"
    );
    let output = match std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
    {
        Ok(output) => output,
        Err(_) => return ProcessIdentity::Unknown,
    };
    match output.status.code() {
        Some(0) => {
            let started = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if started.is_empty() {
                ProcessIdentity::Unknown
            } else {
                ProcessIdentity::Alive(format!("windows-start:{started}"))
            }
        }
        Some(3) => ProcessIdentity::Dead,
        _ => ProcessIdentity::Unknown,
    }
}

#[cfg(not(any(unix, windows)))]
fn process_identity(_pid: u32) -> ProcessIdentity {
    ProcessIdentity::Unknown
}

/// Reads the owner metadata recorded in an engine lock file, if any.
pub fn read_engine_lock_owner(path: &Path) -> Option<EngineLockOwner> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(raw.trim()).ok()
}

/// Process-lifetime guard preventing two local engines from sharing one state root.
#[derive(Debug)]
pub struct StatefulEngineLock {
    file: File,
    path: PathBuf,
}

impl StatefulEngineLock {
    pub fn acquire(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .with_context(|| format!("failed to open engine lock {}", path.display()))?;
        if file.try_lock_exclusive().is_err() {
            bail!("{}", Self::held_lock_diagnostics(path));
        }
        if let Some(previous) = read_engine_lock_owner(path) {
            if previous.pid != std::process::id() {
                match previous.is_alive() {
                    Some(false) => tracing::info!(
                        lock = %path.display(),
                        previous_pid = previous.pid,
                        "recovered stateful engine lock from an owner that exited uncleanly"
                    ),
                    Some(true) => {
                        let _ = FileExt::unlock(&file);
                        bail!(
                            "refusing stateful engine lock takeover at {}: recorded owner pid {} is still alive",
                            path.display(),
                            previous.pid
                        );
                    }
                    None => {
                        let _ = FileExt::unlock(&file);
                        bail!(
                            "refusing stateful engine lock takeover at {}: recorded owner pid {} cannot be checked for liveness on this platform",
                            path.display(),
                            previous.pid
                        );
                    }
                }
            }
        }
        let owner = EngineLockOwner::current(crate::util::time::now_ms());
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(serde_json::to_string(&owner)?.as_bytes())?;
        file.sync_all()?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    fn held_lock_diagnostics(path: &Path) -> String {
        let base = format!(
            "another Tandem engine already owns runtime root lock {}",
            path.display()
        );
        match read_engine_lock_owner(path) {
            Some(owner) => match owner.is_alive() {
                Some(true) => format!(
                    "{base}: held by live engine pid {} since {} - stop that engine before starting another on this runtime root",
                    owner.pid, owner.acquired_at_ms
                ),
                Some(false) => format!(
                    "{base}: recorded owner pid {} is no longer the same live process, so the advisory lock was not released by the filesystem - after confirming no other engine uses this root, delete {} and restart",
                    owner.pid,
                    path.display()
                ),
                None => format!(
                    "{base}: recorded owner pid {} (liveness unknown on this platform) - verify the process before removing {}",
                    owner.pid,
                    path.display()
                ),
            },
            None => format!(
                "{base}: no owner metadata was recorded - verify no other engine uses this root before removing the lock file"
            ),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn owner(&self) -> Option<EngineLockOwner> {
        read_engine_lock_owner(&self.path)
    }
}

impl Drop for StatefulEngineLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::EngineLockOwner;

    #[cfg(any(unix, windows))]
    #[test]
    fn process_identity_distinguishes_current_and_absent_processes() {
        let current = EngineLockOwner::for_pid(std::process::id(), 1);
        assert!(current.process_start_hint.is_some());
        assert_eq!(current.is_alive(), Some(true));

        let absent = EngineLockOwner::for_pid(u32::MAX, 1);
        assert_eq!(absent.is_alive(), Some(false));
    }
}
