// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#![cfg_attr(
    not(test),
    deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)
)]

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{copy, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MARKER_FILE: &str = "tandempack.yaml";
const INDEX_FILE: &str = "index.json";
const CURRENT_FILE: &str = "current";
const STAGING_DIR: &str = ".staging";
const EXPORTS_DIR: &str = "exports";
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 5_000;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PATH_DEPTH: usize = 24;
const MAX_ENTRY_COMPRESSION_RATIO: u64 = 200;
const MAX_ARCHIVE_COMPRESSION_RATIO: u64 = 200;
const SECRET_SCAN_MAX_FILE_BYTES: u64 = 512 * 1024;
const SECRET_SCAN_PATTERNS: &[&str] = &["sk-", "sk_live_", "ghp_", "xoxb-", "xoxp-", "AKIA"];
const PACK_SIGNATURE_FILE: &str = "tandempack.sig";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub pack_type: String,
    #[serde(default)]
    pub manifest_schema_version: Option<String>,
    #[serde(default)]
    pub pack_id: Option<String>,
    #[serde(default)]
    pub marketplace: Option<Value>,
    #[serde(default)]
    pub capabilities: Value,
    #[serde(default)]
    pub entrypoints: Value,
    #[serde(default)]
    pub contents: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInstallRecord {
    pub pack_id: String,
    pub name: String,
    pub version: String,
    pub pack_type: String,
    pub install_path: String,
    pub sha256: String,
    pub installed_at_ms: u64,
    pub source: Value,
    #[serde(default)]
    pub marker_detected: bool,
    #[serde(default)]
    pub routines_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackIndex {
    #[serde(default)]
    pub packs: Vec<PackInstallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInspection {
    pub installed: PackInstallRecord,
    pub manifest: Value,
    pub trust: Value,
    pub risk: Value,
    pub permission_sheet: Value,
    pub workflow_extensions: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInstallRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, alias = "sha256")]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub source: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackUninstallRequest {
    #[serde(default)]
    pub pack_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackExportRequest {
    #[serde(default)]
    pub pack_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackExportResult {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone)]
pub struct PackManager {
    root: PathBuf,
    index_lock: Arc<Mutex<()>>,
    pack_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackSignaturePolicy {
    RequireTrusted,
    AllowUnsignedGenerated,
}

impl PackManager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            index_lock: Arc::new(Mutex::new(())),
            pack_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn default_root() -> PathBuf {
        tandem_core::resolve_shared_paths()
            .map(|paths| paths.canonical_root.join("packs"))
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".tandem")
                    .join("packs")
            })
    }

    pub async fn list(&self) -> anyhow::Result<Vec<PackInstallRecord>> {
        let index = self.read_index().await?;
        Ok(index.packs)
    }

    pub async fn inspect(&self, selector: &str) -> anyhow::Result<PackInspection> {
        let index = self.read_index().await?;
        let Some(installed) = select_record(&index, Some(selector), None) else {
            return Err(anyhow!("pack not found"));
        };
        let install_path = self.validated_record_install_path(&installed)?;
        let manifest_path = install_path.join(MARKER_FILE);
        let manifest_raw = tokio::fs::read_to_string(&manifest_path)
            .await
            .with_context(|| format!("read {}", manifest_path.display()))?;
        let manifest: Value = serde_yaml::from_str(&manifest_raw).context("parse manifest yaml")?;
        let trust = inspect_trust(&manifest, &installed.install_path);
        let risk = inspect_risk(&manifest, &installed);
        let permission_sheet = inspect_permission_sheet(&manifest, &risk);
        let workflow_extensions = inspect_workflow_extensions(&manifest);
        Ok(PackInspection {
            installed,
            manifest,
            trust,
            risk,
            permission_sheet,
            workflow_extensions,
        })
    }

    pub async fn install(&self, input: PackInstallRequest) -> anyhow::Result<PackInstallRecord> {
        self.install_with_signature_policy(input, PackSignaturePolicy::RequireTrusted)
            .await
    }

    pub(crate) fn generated_staging_root(&self) -> PathBuf {
        self.root.join(STAGING_DIR).join("generated")
    }

    pub(crate) async fn install_generated(
        &self,
        path: PathBuf,
        source: Value,
    ) -> anyhow::Result<PackInstallRecord> {
        let generated_root = self.generated_staging_root();
        let canonical_root = tokio::fs::canonicalize(&generated_root)
            .await
            .context("resolve generated pack staging root")?;
        let canonical_path = tokio::fs::canonicalize(&path)
            .await
            .with_context(|| format!("resolve generated pack {}", path.display()))?;
        if canonical_path == canonical_root || !canonical_path.starts_with(&canonical_root) {
            return Err(anyhow!(
                "generated pack must be a file beneath the PackManager staging root"
            ));
        }
        if !tokio::fs::metadata(&canonical_path).await?.is_file() {
            return Err(anyhow!("generated pack must be a regular file"));
        }
        self.install_with_signature_policy(
            PackInstallRequest {
                path: Some(canonical_path.to_string_lossy().to_string()),
                url: None,
                expected_sha256: None,
                source,
            },
            PackSignaturePolicy::AllowUnsignedGenerated,
        )
        .await
    }

    async fn install_with_signature_policy(
        &self,
        input: PackInstallRequest,
        signature_policy: PackSignaturePolicy,
    ) -> anyhow::Result<PackInstallRecord> {
        self.ensure_layout().await?;
        let source_file = if let Some(path) = input.path.as_deref() {
            PathBuf::from(path)
        } else if let Some(url) = input.url.as_deref() {
            self.download_to_staging(url).await?
        } else {
            return Err(anyhow!("install requires either `path` or `url`"));
        };
        let source_meta = tokio::fs::metadata(&source_file)
            .await
            .with_context(|| format!("stat {}", source_file.display()))?;
        if source_meta.len() > MAX_ARCHIVE_BYTES {
            return Err(anyhow!(
                "archive exceeds max size ({} > {})",
                source_meta.len(),
                MAX_ARCHIVE_BYTES
            ));
        }
        if !contains_root_marker(&source_file)? {
            return Err(anyhow!("zip does not contain root marker tandempack.yaml"));
        }
        let manifest = read_manifest_from_zip(&source_file)?;
        let sha256 = sha256_file(&source_file)?;
        if let Some(expected) = input
            .expected_sha256
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if expected.len() != 64
                || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !sha256.eq_ignore_ascii_case(expected)
            {
                return Err(anyhow!("pack archive sha256 digest mismatch"));
            }
        } else if input.url.is_some() {
            return Err(anyhow!(
                "remote pack install requires expected_sha256 (or sha256)"
            ));
        }
        let pack_id = manifest
            .pack_id
            .clone()
            .unwrap_or_else(|| manifest.name.clone());
        let pack_lock = self.pack_lock(&manifest.name).await;
        let _pack_guard = pack_lock.lock().await;

        let stage_id = format!("install-{}", Uuid::new_v4());
        let stage_root = self.root.join(STAGING_DIR).join(stage_id);
        let stage_unpacked = stage_root.join("unpacked");
        tokio::fs::create_dir_all(&stage_unpacked).await?;
        safe_extract_zip(&source_file, &stage_unpacked)?;
        let manifest_value = serde_json::to_value(&manifest)?;
        validate_manifest(&manifest, &manifest_value, &stage_unpacked)?;
        let secret_hits = scan_embedded_secrets(&stage_unpacked)?;
        let strict_secret_scan = std::env::var("TANDEM_PACK_SECRET_SCAN_STRICT")
            .map(|v| {
                let n = v.to_ascii_lowercase();
                n == "1" || n == "true" || n == "yes" || n == "on"
            })
            .unwrap_or(false);
        if strict_secret_scan && !secret_hits.is_empty() {
            let _ = tokio::fs::remove_dir_all(&stage_root).await;
            return Err(anyhow!(
                "embedded_secret_detected: {} potential secret(s) found (first: {})",
                secret_hits.len(),
                secret_hits[0]
            ));
        }
        let signature_status = verify_pack_signature(&stage_unpacked)?;
        if signature_policy == PackSignaturePolicy::RequireTrusted
            && pack_signature_required()
            && matches!(signature_status, PackSignatureStatus::Unsigned)
        {
            let _ = tokio::fs::remove_dir_all(&stage_root).await;
            return Err(anyhow!("pack signature is required"));
        }

        validate_pack_identifier("manifest.pack_id", &pack_id)?;
        let install_target = self.install_path(&manifest.name, &manifest.version)?;
        let install_parent = install_target
            .parent()
            .ok_or_else(|| anyhow!("invalid pack install parent"))?
            .to_path_buf();
        if install_target.exists() {
            let _ = tokio::fs::remove_dir_all(&stage_root).await;
            return Err(anyhow!(
                "pack already installed: {}@{}",
                manifest.name,
                manifest.version
            ));
        }
        tokio::fs::create_dir_all(&install_parent).await?;
        reject_symlink_path(&install_parent, "pack install directory")?;
        tokio::fs::rename(&stage_unpacked, &install_target)
            .await
            .with_context(|| {
                format!(
                    "move {} -> {}",
                    stage_unpacked.display(),
                    install_target.display()
                )
            })?;
        let _ = tokio::fs::remove_dir_all(&stage_root).await;

        let record = PackInstallRecord {
            pack_id,
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            pack_type: manifest.pack_type.clone(),
            install_path: install_target.to_string_lossy().to_string(),
            sha256,
            installed_at_ms: now_ms(),
            source: if input.source.is_null() {
                serde_json::json!({
                    "kind": if input.url.is_some() { "url" } else { "path" },
                    "path": input.path,
                    "url": input.url,
                    "expected_sha256": input.expected_sha256,
                })
            } else {
                input.source
            },
            marker_detected: true,
            routines_enabled: false,
        };
        let index_guard = self.index_lock.lock().await;
        let previous_index = match self.read_index_unlocked().await {
            Ok(index) => index,
            Err(error) => {
                drop(index_guard);
                if let Err(rollback_error) = tokio::fs::remove_dir_all(&install_target).await {
                    return Err(error.context(format!(
                        "read pack index; failed to roll back {}: {rollback_error}",
                        install_target.display()
                    )));
                }
                return Err(error).context("read pack index; installation rolled back");
            }
        };
        let mut next_index = previous_index.clone();
        next_index.packs.retain(|row| {
            !(row.pack_id == record.pack_id
                && row.name == record.name
                && row.version == record.version)
        });
        next_index.packs.push(record.clone());
        if let Err(error) = self.write_index_unlocked(&next_index).await {
            drop(index_guard);
            if let Err(rollback_error) = tokio::fs::remove_dir_all(&install_target).await {
                return Err(error.context(format!(
                    "persist pack index; failed to roll back {}: {rollback_error}",
                    install_target.display()
                )));
            }
            return Err(error).context("persist pack index; installation rolled back");
        }
        if let Err(error) = self
            .write_current_version(&manifest.name, &manifest.version)
            .await
        {
            if let Err(rollback_error) = self.write_index_unlocked(&previous_index).await {
                drop(index_guard);
                return Err(error.context(format!(
                    "commit pack current pointer; failed to roll back index: {rollback_error}; installed files retained"
                )));
            }
            drop(index_guard);
            if let Err(rollback_error) = tokio::fs::remove_dir_all(&install_target).await {
                return Err(error.context(format!(
                    "commit pack current pointer; index rolled back but failed to remove {}: {rollback_error}",
                    install_target.display()
                )));
            }
            return Err(error).context("commit pack current pointer; installation rolled back");
        }
        drop(index_guard);
        Ok(record)
    }

    pub async fn uninstall(&self, req: PackUninstallRequest) -> anyhow::Result<PackInstallRecord> {
        let selector = req.pack_id.as_deref().or(req.name.as_deref());
        let index_snapshot = self.read_index().await?;
        let Some(snapshot_record) =
            select_record(&index_snapshot, selector, req.version.as_deref())
        else {
            return Err(anyhow!("pack not found"));
        };
        let pack_lock = self.pack_lock(&snapshot_record.name).await;
        let _pack_guard = pack_lock.lock().await;

        // Keep the index writer across the final record lookup, staged
        // filesystem move, and atomic index replacement so installs for other
        // pack names cannot be lost through a read/modify/write race.
        let index_guard = self.index_lock.lock().await;
        let mut index = self.read_index_unlocked().await?;
        let previous_index = index.clone();
        let Some(record) = select_record(&index, selector, req.version.as_deref()) else {
            return Err(anyhow!("pack not found"));
        };
        let install_path = self.validated_record_install_path(&record)?;
        let staged_removal = if install_path.exists() {
            self.ensure_layout().await?;
            let staged = self
                .root
                .join(STAGING_DIR)
                .join(format!("uninstall-{}", Uuid::new_v4()));
            tokio::fs::rename(&install_path, &staged)
                .await
                .with_context(|| {
                    format!(
                        "stage pack uninstall {} -> {}",
                        install_path.display(),
                        staged.display()
                    )
                })?;
            Some(staged)
        } else {
            None
        };
        index.packs.retain(|row| {
            !(row.pack_id == record.pack_id
                && row.name == record.name
                && row.version == record.version
                && row.install_path == record.install_path)
        });
        if let Err(error) = self.write_index_unlocked(&index).await {
            if let Some(staged) = staged_removal.as_ref() {
                if let Err(rollback_error) = tokio::fs::rename(staged, &install_path).await {
                    return Err(error.context(format!(
                        "persist pack index; failed to roll back staged uninstall {} -> {}: {rollback_error}",
                        staged.display(),
                        install_path.display()
                    )));
                }
            }
            return Err(error).context("persist pack index; uninstall rolled back");
        }
        if let Err(error) = self
            .write_current_for_index_unlocked(&index, &record.name)
            .await
        {
            if let Err(rollback_error) = self.write_index_unlocked(&previous_index).await {
                drop(index_guard);
                return Err(error.context(format!(
                    "commit pack current pointer; failed to roll back index: {rollback_error}; staged files retained"
                )));
            }
            if let Some(staged) = staged_removal.as_ref() {
                if let Err(rollback_error) = tokio::fs::rename(staged, &install_path).await {
                    drop(index_guard);
                    return Err(error.context(format!(
                        "commit pack current pointer; index rolled back but failed to restore {} -> {}: {rollback_error}",
                        staged.display(),
                        install_path.display()
                    )));
                }
            }
            drop(index_guard);
            return Err(error).context("commit pack current pointer; uninstall rolled back");
        }
        drop(index_guard);
        if let Some(staged) = staged_removal {
            let _ = tokio::fs::remove_dir_all(staged).await;
        }
        Ok(record)
    }

    pub async fn export(&self, req: PackExportRequest) -> anyhow::Result<PackExportResult> {
        let index = self.read_index().await?;
        let selector = req.pack_id.as_deref().or(req.name.as_deref());
        let Some(record) = select_record(&index, selector, req.version.as_deref()) else {
            return Err(anyhow!("pack not found"));
        };
        let pack_dir = self.validated_record_install_path(&record)?;
        if !pack_dir.exists() {
            return Err(anyhow!("installed pack path missing"));
        }
        reject_symlink_path(&pack_dir, "installed pack")?;
        let output = if let Some(path) = req.output_path {
            let path = Path::new(path.trim());
            let mut components = path.components();
            let file_name = match (components.next(), components.next()) {
                (Some(Component::Normal(file_name)), None) => file_name,
                _ => return Err(anyhow!("pack export output_path must be a safe file name")),
            };
            let file_name = file_name
                .to_str()
                .ok_or_else(|| anyhow!("pack export file name must be UTF-8"))?;
            validate_export_file_name(file_name)?;
            self.root.join(EXPORTS_DIR).join(file_name)
        } else {
            self.root
                .join(EXPORTS_DIR)
                .join(format!("{}-{}.zip", record.name, record.version))
        };
        let parent = output
            .parent()
            .ok_or_else(|| anyhow!("pack export output has no parent"))?;
        tokio::fs::create_dir_all(parent).await?;
        reject_symlink_path(parent, "pack export directory")?;
        let temporary = parent.join(format!(".export-{}.tmp", Uuid::new_v4()));
        if let Err(error) = zip_directory(&pack_dir, &temporary) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = std::fs::hard_link(&temporary, &output) {
            let _ = std::fs::remove_file(&temporary);
            return Err(anyhow!(
                "create pack export without overwriting an existing file: {error}"
            ));
        }
        let _ = std::fs::remove_file(&temporary);
        let bytes = tokio::fs::metadata(&output).await?.len();
        Ok(PackExportResult {
            path: output.to_string_lossy().to_string(),
            sha256: sha256_file(&output)?,
            bytes,
        })
    }

    pub async fn detect(&self, path: &Path) -> anyhow::Result<bool> {
        Ok(contains_root_marker(path)?)
    }

    async fn download_to_staging(&self, url: &str) -> anyhow::Result<PathBuf> {
        self.ensure_layout().await?;
        let stage = self
            .root
            .join(STAGING_DIR)
            .join(format!("download-{}.zip", Uuid::new_v4()));
        let target = crate::outbound_http::resolve_public_https_url(url).await?;
        let client = target.client(Duration::from_secs(30))?;
        let mut response = client
            .get(target.url().clone())
            .send()
            .await
            .with_context(|| format!("download {}", url))?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "pack download returned HTTP {}",
                response.status().as_u16()
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
        {
            return Err(anyhow!("pack download exceeds max archive size"));
        }
        let mut output = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .await
            .with_context(|| format!("create staging download {}", stage.display()))?;
        let mut written = 0u64;
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) => {
                    drop(output);
                    let _ = tokio::fs::remove_file(&stage).await;
                    return Err(error).context("read pack download");
                }
            };
            written = written.saturating_add(chunk.len() as u64);
            if written > MAX_ARCHIVE_BYTES {
                drop(output);
                let _ = tokio::fs::remove_file(&stage).await;
                return Err(anyhow!("pack download exceeds max archive size"));
            }
            if let Err(error) = output.write_all(&chunk).await {
                drop(output);
                let _ = tokio::fs::remove_file(&stage).await;
                return Err(error).context("write pack staging download");
            }
        }
        if let Err(error) = output.flush().await {
            drop(output);
            let _ = tokio::fs::remove_file(&stage).await;
            return Err(error).context("flush pack staging download");
        }
        if let Err(error) = output.sync_all().await {
            drop(output);
            let _ = tokio::fs::remove_file(&stage).await;
            return Err(error).context("sync pack staging download");
        }
        Ok(stage)
    }

    fn install_path(&self, name: &str, version: &str) -> anyhow::Result<PathBuf> {
        validate_pack_identifier("manifest.name", name)?;
        validate_pack_identifier("manifest.version", version)?;
        Ok(self.root.join(name).join(version))
    }

    fn validated_record_install_path(&self, record: &PackInstallRecord) -> anyhow::Result<PathBuf> {
        validate_pack_identifier("pack_id", &record.pack_id)?;
        let expected = self.install_path(&record.name, &record.version)?;
        if Path::new(&record.install_path) != expected {
            return Err(anyhow!(
                "pack index install path is outside its rooted identity"
            ));
        }
        if expected.exists() {
            reject_symlink_path(&expected, "installed pack")?;
        }
        Ok(expected)
    }

    async fn ensure_layout(&self) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.root).await?;
        tokio::fs::create_dir_all(self.root.join(STAGING_DIR)).await?;
        tokio::fs::create_dir_all(self.root.join(EXPORTS_DIR)).await?;
        reject_symlink_path(&self.root, "pack root")?;
        reject_symlink_path(&self.root.join(STAGING_DIR), "pack staging directory")?;
        reject_symlink_path(&self.root.join(EXPORTS_DIR), "pack export directory")?;
        Ok(())
    }

    async fn read_index(&self) -> anyhow::Result<PackIndex> {
        let _index_guard = self.index_lock.lock().await;
        self.read_index_unlocked().await
    }

    async fn write_index(&self, index: &PackIndex) -> anyhow::Result<()> {
        let _index_guard = self.index_lock.lock().await;
        self.write_index_unlocked(index).await
    }

    async fn read_index_unlocked(&self) -> anyhow::Result<PackIndex> {
        let index_path = self.root.join(INDEX_FILE);
        if !index_path.exists() {
            return Ok(PackIndex::default());
        }
        let raw = tokio::fs::read_to_string(&index_path)
            .await
            .with_context(|| format!("read {}", index_path.display()))?;
        let parsed = serde_json::from_str::<PackIndex>(&raw).unwrap_or_default();
        Ok(parsed)
    }

    async fn write_index_unlocked(&self, index: &PackIndex) -> anyhow::Result<()> {
        self.ensure_layout().await?;
        let index_path = self.root.join(INDEX_FILE);
        let tmp = self
            .root
            .join(format!("{}.{}.tmp", INDEX_FILE, Uuid::new_v4()));
        let payload = serde_json::to_string_pretty(index)?;
        tokio::fs::write(&tmp, format!("{}\n", payload)).await?;
        tokio::fs::rename(&tmp, &index_path).await?;
        Ok(())
    }

    async fn write_current_for_index_unlocked(
        &self,
        index: &PackIndex,
        pack_name: &str,
    ) -> anyhow::Result<()> {
        let mut versions = index
            .packs
            .iter()
            .filter(|row| row.name == pack_name)
            .collect::<Vec<_>>();
        versions.sort_by(|a, b| b.installed_at_ms.cmp(&a.installed_at_ms));
        let current_path = self.root.join(pack_name).join(CURRENT_FILE);
        if let Some(latest) = versions.first() {
            self.write_current_version(pack_name, &latest.version)
                .await?;
        } else {
            match tokio::fs::symlink_metadata(&current_path).await {
                Ok(metadata) if metadata.file_type().is_file() => {
                    tokio::fs::remove_file(&current_path)
                        .await
                        .context("remove pack current pointer")?;
                }
                Ok(_) => return Err(anyhow!("pack current pointer is not a regular file")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("inspect pack current pointer"),
            }
        }
        Ok(())
    }

    async fn write_current_version(&self, pack_name: &str, version: &str) -> anyhow::Result<()> {
        validate_pack_identifier("manifest.name", pack_name)?;
        validate_pack_identifier("manifest.version", version)?;
        let parent = self.root.join(pack_name);
        tokio::fs::create_dir_all(&parent).await?;
        reject_symlink_path(&parent, "pack current directory")?;
        let current_path = parent.join(CURRENT_FILE);
        let temporary = parent.join(format!(".{CURRENT_FILE}.{}.tmp", Uuid::new_v4()));
        let mut output = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await?;
        if let Err(error) = output.write_all(format!("{version}\n").as_bytes()).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error).context("write pack current pointer");
        }
        if let Err(error) = output.sync_all().await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error).context("sync pack current pointer");
        }
        drop(output);

        match tokio::fs::symlink_metadata(&current_path).await {
            Ok(metadata) if metadata.file_type().is_file() => {}
            Ok(_) => {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(anyhow!("pack current pointer is not a regular file"));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(error).context("inspect previous pack current pointer");
            }
        }

        #[cfg(not(windows))]
        {
            if let Err(error) = tokio::fs::rename(&temporary, &current_path).await {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(error).context("commit pack current pointer");
            }
            return Ok(());
        }

        #[cfg(windows)]
        {
            let backup = parent.join(format!(".{CURRENT_FILE}.{}.bak", Uuid::new_v4()));
            let previous_moved = match tokio::fs::symlink_metadata(&current_path).await {
                Ok(metadata) if metadata.file_type().is_file() => {
                    if let Err(error) = tokio::fs::rename(&current_path, &backup).await {
                        let _ = tokio::fs::remove_file(&temporary).await;
                        return Err(error).context("stage previous pack current pointer");
                    }
                    true
                }
                Ok(_) => {
                    let _ = tokio::fs::remove_file(&temporary).await;
                    return Err(anyhow!("pack current pointer is not a regular file"));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temporary).await;
                    return Err(error).context("inspect previous pack current pointer");
                }
            };
            if let Err(error) = tokio::fs::rename(&temporary, &current_path).await {
                let _ = tokio::fs::remove_file(&temporary).await;
                if previous_moved {
                    if let Err(rollback_error) = tokio::fs::rename(&backup, &current_path).await {
                        return Err(error).context(format!(
                        "commit pack current pointer; failed to restore previous pointer: {rollback_error}"
                    ));
                    }
                }
                return Err(error).context("commit pack current pointer");
            }
            if previous_moved {
                let _ = tokio::fs::remove_file(&backup).await;
            }
            Ok(())
        }
    }

    async fn pack_lock(&self, pack_name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.pack_locks.lock().await;
        locks
            .entry(pack_name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn select_record<'a>(
    index: &'a PackIndex,
    selector: Option<&str>,
    version: Option<&str>,
) -> Option<PackInstallRecord> {
    let selector = selector.map(|s| s.trim()).filter(|s| !s.is_empty());
    let mut matches = index
        .packs
        .iter()
        .filter(|row| match selector {
            Some(sel) => row.pack_id == sel || row.name == sel,
            None => true,
        })
        .filter(|row| match version {
            Some(version) => row.version == version,
            None => true,
        })
        .cloned()
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| b.installed_at_ms.cmp(&a.installed_at_ms));
    matches.into_iter().next()
}

fn contains_root_marker(path: &Path) -> anyhow::Result<bool> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut archive = ZipArchive::new(file).context("open zip archive")?;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).context("read zip entry")?;
        if entry.name() == MARKER_FILE {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_manifest_from_zip(path: &Path) -> anyhow::Result<PackManifest> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut archive = ZipArchive::new(file).context("open zip archive")?;
    let mut manifest_file = archive
        .by_name(MARKER_FILE)
        .context("missing root tandempack.yaml")?;
    let mut text = String::new();
    manifest_file.read_to_string(&mut text)?;
    let manifest = serde_yaml::from_str::<PackManifest>(&text).context("parse manifest yaml")?;
    Ok(manifest)
}

fn validate_manifest(
    manifest: &PackManifest,
    manifest_value: &Value,
    install_root: &Path,
) -> anyhow::Result<()> {
    validate_pack_identifier("manifest.name", &manifest.name)?;
    validate_pack_identifier("manifest.version", &manifest.version)?;
    validate_pack_identifier("manifest.type", &manifest.pack_type)?;
    if let Some(pack_id) = manifest.pack_id.as_deref() {
        validate_pack_identifier("manifest.pack_id", pack_id)?;
    }
    if let Some(marketplace) = manifest_value
        .pointer("/marketplace")
        .and_then(|value| value.as_object())
    {
        validate_marketplace_metadata(marketplace)?;
        validate_manifest_references(manifest_value, install_root)?;
    }
    Ok(())
}

fn validate_marketplace_metadata(
    marketplace: &serde_json::Map<String, Value>,
) -> anyhow::Result<()> {
    let publisher = marketplace
        .get("publisher")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow!("marketplace.publisher is required"))?;
    for key in ["publisher_id", "display_name", "verification_tier"] {
        if publisher
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| !value.trim().is_empty())
            != Some(true)
        {
            return Err(anyhow!("marketplace.publisher.{key} is required"));
        }
    }

    let listing = marketplace
        .get("listing")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow!("marketplace.listing is required"))?;
    for key in ["display_name", "description", "license_spdx"] {
        if listing
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| !value.trim().is_empty())
            != Some(true)
        {
            return Err(anyhow!("marketplace.listing.{key} is required"));
        }
    }
    if listing
        .get("categories")
        .and_then(|value| value.as_array())
        .map(|rows| rows.is_empty())
        .unwrap_or(true)
    {
        return Err(anyhow!("marketplace.listing.categories is required"));
    }
    if listing
        .get("tags")
        .and_then(|value| value.as_array())
        .map(|rows| rows.is_empty())
        .unwrap_or(true)
    {
        return Err(anyhow!("marketplace.listing.tags is required"));
    }
    Ok(())
}

fn validate_manifest_references(manifest_value: &Value, install_root: &Path) -> anyhow::Result<()> {
    let mut references = Vec::new();
    if let Some(contents) = manifest_value.pointer("/contents") {
        collect_manifest_paths(contents, &mut references);
    }
    if let Some(listing) = manifest_value.pointer("/marketplace/listing") {
        for field in ["icon", "cover_image", "changelog"] {
            if let Some(path) = listing.get(field).and_then(|value| value.as_str()) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    references.push(trimmed.to_string());
                }
            }
        }
        if let Some(items) = listing
            .get("screenshots")
            .and_then(|value| value.as_array())
        {
            for item in items {
                if let Some(path) = item.as_str() {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        references.push(trimmed.to_string());
                    }
                }
            }
        }
    }
    references.sort();
    references.dedup();
    for rel in references {
        let rel = safe_relative_pack_path(&rel)?;
        let path = install_root.join(&rel);
        if !path.exists() {
            return Err(anyhow!("declared pack file missing: {}", path.display()));
        }
    }
    Ok(())
}

fn validate_pack_identifier(field: &str, value: &str) -> anyhow::Result<()> {
    let value = value.trim();
    let valid = (1..=128).contains(&value.len())
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
        && !value.contains("..");
    if !valid {
        return Err(anyhow!("{field} contains an unsafe identifier"));
    }
    Ok(())
}

fn safe_relative_pack_path(value: &str) -> anyhow::Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty() || value.contains('\0') {
        return Err(anyhow!("invalid pack-relative path"));
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => out.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("unsafe pack-relative path: {value}"));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(anyhow!("invalid pack-relative path"));
    }
    Ok(out)
}

fn validate_export_file_name(value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || value.len() > 180
        || !value.ends_with(".zip")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || value.contains("..")
    {
        return Err(anyhow!(
            "pack export output_path must be a safe .zip file name"
        ));
    }
    Ok(())
}

fn collect_manifest_paths(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Array(rows) => {
            for row in rows {
                collect_manifest_paths(row, out);
            }
        }
        Value::Object(map) => {
            if let Some(path) = map.get("path").and_then(|value| value.as_str()) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
            for child in map.values() {
                collect_manifest_paths(child, out);
            }
        }
        _ => {}
    }
}

fn safe_extract_zip(zip_path: &Path, out_dir: &Path) -> anyhow::Result<()> {
    let file = File::open(zip_path).with_context(|| format!("open {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).context("open zip archive")?;
    if archive.len() > MAX_FILES {
        return Err(anyhow!(
            "zip has too many entries ({} > {})",
            archive.len(),
            MAX_FILES
        ));
    }
    let mut extracted_files = 0usize;
    let mut extracted_total = 0u64;
    let mut compressed_total = 0u64;
    let mut extracted_paths = HashSet::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).context("zip entry read")?;
        let entry_name = entry.name().to_string();
        validate_zip_entry_name(&entry_name)?;
        let out_path = out_dir.join(safe_relative_pack_path(&entry_name)?);
        if !extracted_paths.insert(out_path.clone()) {
            return Err(anyhow!("zip contains duplicate entry path: {entry_name}"));
        }
        if entry_name.ends_with('/') {
            continue;
        }
        let size = entry.size();
        let compressed_size = entry.compressed_size().max(1);
        let entry_ratio = size.saturating_div(compressed_size);
        if entry_ratio > MAX_ENTRY_COMPRESSION_RATIO {
            return Err(anyhow!(
                "zip entry compression ratio too high: {} ({}/{})",
                entry_name,
                size,
                compressed_size
            ));
        }
        if size > MAX_FILE_BYTES {
            return Err(anyhow!(
                "zip entry exceeds max size: {} ({} > {})",
                entry_name,
                size,
                MAX_FILE_BYTES
            ));
        }
        extracted_files = extracted_files.saturating_add(1);
        if extracted_files > MAX_FILES {
            return Err(anyhow!(
                "zip has too many files ({} > {})",
                extracted_files,
                MAX_FILES
            ));
        }
        extracted_total = extracted_total.saturating_add(size);
        if extracted_total > MAX_EXTRACTED_BYTES {
            return Err(anyhow!(
                "zip extracted bytes exceed max ({} > {})",
                extracted_total,
                MAX_EXTRACTED_BYTES
            ));
        }
        compressed_total = compressed_total.saturating_add(compressed_size);
        let archive_ratio_ceiling = compressed_total.saturating_mul(MAX_ARCHIVE_COMPRESSION_RATIO);
        if extracted_total > archive_ratio_ceiling {
            return Err(anyhow!(
                "zip archive compression ratio too high (extracted={} compressed={})",
                extracted_total,
                compressed_total
            ));
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        let mut outfile =
            File::create(&out_path).with_context(|| format!("create {}", out_path.display()))?;
        let mut limited = entry.take(MAX_FILE_BYTES + 1);
        let written = copy(&mut limited, &mut outfile)?;
        if written > MAX_FILE_BYTES {
            return Err(anyhow!(
                "zip entry exceeded max copied bytes: {}",
                entry_name
            ));
        }
    }
    Ok(())
}

fn validate_zip_entry_name(name: &str) -> anyhow::Result<()> {
    if name.starts_with('/') || name.starts_with('\\') || name.contains('\0') {
        return Err(anyhow!("invalid zip entry path: {}", name));
    }
    let path = Path::new(name);
    let mut depth = 0usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => {
                depth = depth.saturating_add(1);
                if depth > MAX_PATH_DEPTH {
                    return Err(anyhow!("zip entry path too deep: {}", name));
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!("unsafe zip entry path: {}", name));
            }
        }
    }
    Ok(())
}

fn reject_symlink_path(path: &Path, label: &str) -> anyhow::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anyhow!("{label} contains a symbolic link"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("inspect {label} path {}", current.display()));
            }
        }
    }
    Ok(())
}

fn zip_directory(src_dir: &Path, output_zip: &Path) -> anyhow::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_zip)
        .with_context(|| format!("create {}", output_zip.display()))?;
    let mut writer = ZipWriter::new(file);
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut stack = vec![src_dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current)?
            .filter_map(|entry| entry.ok())
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(anyhow!(
                    "pack export refuses symbolic link: {}",
                    path.display()
                ));
            }
            let rel = path
                .strip_prefix(src_dir)
                .context("strip source prefix")?
                .to_string_lossy()
                .replace('\\', "/");
            if file_type.is_dir() {
                if !rel.is_empty() {
                    writer.add_directory(format!("{}/", rel), opts)?;
                }
                stack.push(path);
                continue;
            }
            let mut input = File::open(&path)?;
            writer.start_file(rel, opts)?;
            copy(&mut input, &mut writer)?;
        }
    }
    writer.finish()?;
    Ok(())
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn scan_embedded_secrets(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut findings = Vec::new();
    for path in walk_files(root)? {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .to_string();
        let rel_lower = rel.to_ascii_lowercase();
        if rel_lower.contains(".example") || rel_lower.ends_with("secrets.example.env") {
            continue;
        }
        let meta = std::fs::metadata(&path)?;
        if meta.len() == 0 || meta.len() > SECRET_SCAN_MAX_FILE_BYTES {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        if bytes.contains(&0) {
            continue;
        }
        let content = String::from_utf8_lossy(&bytes);
        for needle in SECRET_SCAN_PATTERNS {
            if content.contains(needle) {
                findings.push(format!("{rel}:{needle}"));
                break;
            }
        }
    }
    Ok(findings)
}

fn walk_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let ty = entry.file_type()?;
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() {
                out.push(path);
            }
        }
    }
    Ok(out)
}

fn inspect_trust(manifest: &Value, install_path: &str) -> Value {
    let (signature, signature_key_id) = match verify_pack_signature(Path::new(install_path)) {
        Ok(PackSignatureStatus::Unsigned) => ("unsigned", None),
        Ok(PackSignatureStatus::Verified { key_id }) => ("verified", Some(key_id)),
        Err(_) => ("invalid", None),
    };
    let publisher_verification = manifest
        .pointer("/publisher/verification")
        .or_else(|| manifest.pointer("/publisher/verification_tier"))
        .or_else(|| manifest.pointer("/marketplace/publisher_verification"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let publisher_verification_normalized =
        match publisher_verification.to_ascii_lowercase().as_str() {
            "official" => "official",
            "verified" => "verified",
            _ => "unverified",
        };
    let verification_badge = match publisher_verification_normalized {
        "official" => "official",
        "verified" => "verified",
        _ => "unverified",
    };
    serde_json::json!({
        "publisher_verification": publisher_verification_normalized,
        "verification_badge": verification_badge,
        "signature": signature,
        "signature_key_id": signature_key_id,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PackSignatureStatus {
    Unsigned,
    Verified { key_id: String },
}

#[derive(Debug, Deserialize)]
struct PackSignatureEnvelope {
    key_id: String,
    signature: String,
}

fn pack_signature_required() -> bool {
    true
}

fn verify_pack_signature(root: &Path) -> anyhow::Result<PackSignatureStatus> {
    let signature_path = root.join(PACK_SIGNATURE_FILE);
    if !signature_path.exists() {
        return Ok(PackSignatureStatus::Unsigned);
    }
    let envelope: PackSignatureEnvelope = serde_json::from_slice(
        &std::fs::read(&signature_path)
            .with_context(|| format!("read {}", signature_path.display()))?,
    )
    .context("parse tandempack.sig JSON")?;
    validate_pack_identifier("signature.key_id", &envelope.key_id)?;
    let trusted_keys = trusted_pack_public_keys()?;
    let public_key = trusted_keys
        .get(&envelope.key_id)
        .ok_or_else(|| anyhow!("pack signature key is not trusted"))?;
    let signature_bytes = decode_base64(&envelope.signature)
        .and_then(|bytes| <[u8; 64]>::try_from(bytes).ok())
        .ok_or_else(|| anyhow!("pack signature must be a base64 Ed25519 signature"))?;
    let verifying_key =
        VerifyingKey::from_bytes(public_key).context("invalid trusted pack public key")?;
    let digest = pack_content_digest(root)?;
    verifying_key
        .verify_strict(&digest, &Signature::from_bytes(&signature_bytes))
        .context("pack signature verification failed")?;
    Ok(PackSignatureStatus::Verified {
        key_id: envelope.key_id,
    })
}

fn trusted_pack_public_keys() -> anyhow::Result<std::collections::BTreeMap<String, [u8; 32]>> {
    let raw = std::env::var("TANDEM_PACK_TRUSTED_PUBLIC_KEYS").unwrap_or_default();
    if raw.trim().is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let entries = if raw.trim_start().starts_with('{') {
        serde_json::from_str::<std::collections::BTreeMap<String, String>>(&raw)
            .context("parse TANDEM_PACK_TRUSTED_PUBLIC_KEYS JSON")?
    } else {
        raw.split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let (key_id, public_key) = entry
                    .split_once('=')
                    .ok_or_else(|| anyhow!("trusted pack key must be key_id=base64_public_key"))?;
                Ok((key_id.trim().to_string(), public_key.trim().to_string()))
            })
            .collect::<anyhow::Result<std::collections::BTreeMap<_, _>>>()?
    };
    entries
        .into_iter()
        .map(|(key_id, encoded)| {
            validate_pack_identifier("trusted pack key id", &key_id)?;
            let bytes = decode_base64(&encoded)
                .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
                .ok_or_else(|| anyhow!("trusted pack public key must decode to 32 bytes"))?;
            Ok((key_id, bytes))
        })
        .collect()
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ]
    .into_iter()
    .find_map(|engine| engine.decode(value.trim()).ok())
}

fn pack_content_digest(root: &Path) -> anyhow::Result<[u8; 32]> {
    let mut files = walk_files(root)?
        .into_iter()
        .filter_map(|path| {
            let rel = path.strip_prefix(root).ok()?.to_path_buf();
            (rel != Path::new(PACK_SIGNATURE_FILE)).then_some((rel, path))
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (rel, path) in files {
        let rel = rel
            .to_str()
            .ok_or_else(|| anyhow!("pack signature path must be UTF-8"))?
            .replace('\\', "/");
        let bytes = std::fs::read(&path)?;
        hasher.update((rel.len() as u64).to_be_bytes());
        hasher.update(rel.as_bytes());
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    Ok(hasher.finalize().into())
}

fn inspect_risk(manifest: &Value, installed: &PackInstallRecord) -> Value {
    let required_capabilities_count = manifest
        .pointer("/capabilities/required")
        .and_then(|v| v.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);
    let optional_capabilities_count = manifest
        .pointer("/capabilities/optional")
        .and_then(|v| v.as_array())
        .map(|rows| rows.len())
        .unwrap_or(0);
    let routines_declared = manifest
        .pointer("/contents/routines")
        .and_then(|v| v.as_array())
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);
    let workflows_declared = manifest
        .pointer("/contents/workflows")
        .and_then(|v| v.as_array())
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);
    let workflow_hooks_declared = manifest
        .pointer("/contents/workflow_hooks")
        .and_then(|v| v.as_array())
        .map(|rows| !rows.is_empty())
        .unwrap_or(false);
    let non_portable_dependencies = manifest
        .pointer("/capabilities/provider_specific")
        .map(|v| match v {
            Value::Array(rows) => !rows.is_empty(),
            Value::Object(map) => !map.is_empty(),
            Value::Bool(flag) => *flag,
            _ => false,
        })
        .unwrap_or(false);
    serde_json::json!({
        "routines_enabled": installed.routines_enabled,
        "routines_declared": routines_declared,
        "workflows_declared": workflows_declared,
        "workflow_hooks_declared": workflow_hooks_declared,
        "required_capabilities_count": required_capabilities_count,
        "optional_capabilities_count": optional_capabilities_count,
        "non_portable_dependencies": non_portable_dependencies,
    })
}

fn inspect_permission_sheet(manifest: &Value, risk: &Value) -> Value {
    let required_capabilities = manifest
        .pointer("/capabilities/required")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let optional_capabilities = manifest
        .pointer("/capabilities/optional")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let provider_specific = manifest
        .pointer("/capabilities/provider_specific")
        .map(|v| match v {
            Value::Array(rows) => rows.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    let routines = manifest
        .pointer("/contents/routines")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let workflows = manifest
        .pointer("/contents/workflows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let workflow_hooks = manifest
        .pointer("/contents/workflow_hooks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    serde_json::json!({
        "required_capabilities": required_capabilities,
        "optional_capabilities": optional_capabilities,
        "provider_specific_dependencies": provider_specific,
        "routines_declared": routines,
        "workflows_declared": workflows,
        "workflow_hooks_declared": workflow_hooks,
        "routines_enabled": risk.get("routines_enabled").cloned().unwrap_or(Value::Bool(false)),
        "risk_level": if !provider_specific.is_empty() { "elevated" } else { "standard" },
    })
}

fn inspect_workflow_extensions(manifest: &Value) -> Value {
    let workflow_entrypoints = manifest
        .pointer("/entrypoints/workflows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let workflows = manifest
        .pointer("/contents/workflows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let workflow_hooks = manifest
        .pointer("/contents/workflow_hooks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    serde_json::json!({
        "workflow_entrypoints": workflow_entrypoints,
        "workflows": workflows,
        "workflow_hooks": workflow_hooks,
        "workflow_count": workflows.len(),
        "workflow_hook_count": workflow_hooks.len(),
    })
}

#[allow(dead_code)]
pub fn map_missing_capability_error(
    workflow_id: &str,
    missing_capabilities: &[String],
    available_capability_bindings: &HashMap<String, Vec<String>>,
) -> Value {
    let suggestions = missing_capabilities
        .iter()
        .map(|cap| {
            let bindings = available_capability_bindings
                .get(cap)
                .cloned()
                .unwrap_or_default();
            serde_json::json!({
                "capability_id": cap,
                "available_bindings": bindings,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "code": "missing_capability",
        "workflow_id": workflow_id,
        "missing_capabilities": missing_capabilities,
        "suggestions": suggestions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::io::Write;

    fn write_zip(path: &Path, entries: &[(&str, &str)]) {
        let file = File::create(path).expect("create zip");
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, body) in entries {
            zip.start_file(*name, opts).expect("start");
            zip.write_all(body.as_bytes()).expect("write");
        }
        zip.finish().expect("finish");
    }

    fn write_signed_zip(path: &Path, entries: &[(&str, &str)]) -> String {
        let mut ordered = entries
            .iter()
            .map(|(name, body)| ((*name).to_string(), body.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        ordered.sort_by(|left, right| left.0.cmp(&right.0));
        let mut hasher = Sha256::new();
        for (name, body) in &ordered {
            hasher.update((name.len() as u64).to_be_bytes());
            hasher.update(name.as_bytes());
            hasher.update((body.len() as u64).to_be_bytes());
            hasher.update(body);
        }
        let digest: [u8; 32] = hasher.finalize().into();
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let signature = signing_key.sign(&digest);
        let envelope = serde_json::json!({
            "key_id": "test-publisher",
            "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
        })
        .to_string();

        let file = File::create(path).expect("create signed zip");
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, body) in entries {
            zip.start_file(*name, opts).expect("start");
            zip.write_all(body.as_bytes()).expect("write");
        }
        zip.start_file(PACK_SIGNATURE_FILE, opts)
            .expect("start signature");
        zip.write_all(envelope.as_bytes()).expect("write signature");
        zip.finish().expect("finish");

        format!(
            "test-publisher={}",
            base64::engine::general_purpose::STANDARD
                .encode(signing_key.verifying_key().to_bytes())
        )
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_deref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn detects_root_marker_only() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let ok = root.join("ok.zip");
        write_zip(
            &ok,
            &[
                ("tandempack.yaml", "name: x\nversion: 1.0.0\ntype: skill\n"),
                ("README.md", "# x"),
            ],
        );
        let nested = root.join("nested.zip");
        write_zip(
            &nested,
            &[(
                "sub/tandempack.yaml",
                "name: x\nversion: 1.0.0\ntype: skill\n",
            )],
        );
        assert!(contains_root_marker(&ok).expect("detect"));
        assert!(!contains_root_marker(&nested).expect("detect nested"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn safe_extract_blocks_traversal() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let bad = root.join("bad.zip");
        write_zip(&bad, &[("../escape.txt", "x")]);
        let out = root.join("out");
        std::fs::create_dir_all(&out).expect("mkdir out");
        let err = safe_extract_zip(&bad, &out).expect_err("should fail");
        assert!(err.to_string().contains("unsafe zip entry path"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn safe_extract_rejects_duplicate_entry_paths() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let bad = root.join("duplicate.zip");
        write_zip(
            &bad,
            &[("payload.txt", "first"), ("./payload.txt", "second")],
        );
        let out = root.join("out");
        std::fs::create_dir_all(&out).expect("mkdir out");
        let error = safe_extract_zip(&bad, &out).expect_err("duplicate paths must fail");
        assert!(error.to_string().contains("duplicate entry path"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn safe_extract_blocks_extreme_compression_ratio() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let bad = root.join("bomb.zip");
        let repeated = "A".repeat(300_000);
        write_zip(&bad, &[("payload.txt", repeated.as_str())]);
        let out = root.join("out");
        std::fs::create_dir_all(&out).expect("mkdir out");
        let err = safe_extract_zip(&bad, &out).expect_err("should fail");
        assert!(err.to_string().contains("compression ratio"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[serial_test::serial(pack_signature_env)]
    async fn inspect_reports_signature_and_risk_summary() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("inspect.zip");
        let trusted_key = write_signed_zip(
            &pack_zip,
            &[
                (
                    "tandempack.yaml",
                    "name: inspect-pack\nversion: 1.0.0\ntype: workflow\npack_id: inspect-pack\npublisher:\n  verification: verified\nentrypoints:\n  workflows:\n    - build_feature\ncapabilities:\n  required:\n    - github.create_pull_request\n  optional:\n    - slack.post_message\ncontents:\n  routines:\n    - routines/nightly.yaml\n  workflows:\n    - id: build_feature\n      path: workflows/build_feature.yaml\n  workflow_hooks:\n    - id: build_feature.task_completed.notify\n      path: hooks/notify.yaml\n",
                ),
                ("routines/nightly.yaml", "id: nightly\n"),
            ],
        );
        let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
        let manager = PackManager::new(root.join("packs"));
        let installed = manager
            .install(PackInstallRequest {
                path: Some(pack_zip.to_string_lossy().to_string()),
                url: None,
                expected_sha256: None,
                source: Value::Null,
            })
            .await
            .expect("install");
        let inspection = manager.inspect(&installed.pack_id).await.expect("inspect");
        assert_eq!(
            inspection.trust.get("signature").and_then(|v| v.as_str()),
            Some("verified")
        );
        assert_eq!(
            inspection
                .trust
                .get("publisher_verification")
                .and_then(|v| v.as_str()),
            Some("verified")
        );
        assert_eq!(
            inspection
                .trust
                .get("verification_badge")
                .and_then(|v| v.as_str()),
            Some("verified")
        );
        assert_eq!(
            inspection
                .risk
                .get("required_capabilities_count")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(
            inspection
                .risk
                .get("routines_declared")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            inspection
                .permission_sheet
                .get("required_capabilities")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            inspection
                .permission_sheet
                .get("routines_declared")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            inspection
                .risk
                .get("workflows_declared")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            inspection
                .risk
                .get("workflow_hooks_declared")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            inspection
                .permission_sheet
                .get("workflows_declared")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            inspection
                .permission_sheet
                .get("workflow_hooks_declared")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            inspection
                .workflow_extensions
                .get("workflow_entrypoints")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(1)
        );
        assert_eq!(
            inspection
                .workflow_extensions
                .get("workflow_count")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(
            inspection
                .workflow_extensions
                .get("workflow_hook_count")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[serial_test::serial(pack_signature_env)]
    async fn inspect_defaults_verification_badge_to_unverified() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("inspect-unverified.zip");
        let trusted_key = write_signed_zip(
            &pack_zip,
            &[(
                "tandempack.yaml",
                "name: inspect-pack-2\nversion: 1.0.0\ntype: workflow\npack_id: inspect-pack-2\n",
            )],
        );
        let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
        let manager = PackManager::new(root.join("packs"));
        let installed = manager
            .install(PackInstallRequest {
                path: Some(pack_zip.to_string_lossy().to_string()),
                url: None,
                expected_sha256: None,
                source: Value::Null,
            })
            .await
            .expect("install");
        let inspection = manager.inspect(&installed.pack_id).await.expect("inspect");
        assert_eq!(
            inspection
                .trust
                .get("verification_badge")
                .and_then(|v| v.as_str()),
            Some("unverified")
        );
        assert_eq!(
            inspection.trust.get("signature").and_then(|v| v.as_str()),
            Some("verified")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn install_rejects_unsigned_pack_by_default() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("unsigned.zip");
        write_zip(
            &pack_zip,
            &[(
                "tandempack.yaml",
                "name: unsigned-pack\nversion: 1.0.0\ntype: workflow\n",
            )],
        );
        let manager = PackManager::new(root.join("packs"));
        let error = manager
            .install(PackInstallRequest {
                path: Some(pack_zip.to_string_lossy().to_string()),
                url: None,
                expected_sha256: None,
                source: Value::Null,
            })
            .await
            .expect_err("unsigned pack must fail");
        assert!(error.to_string().contains("signature is required"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[serial_test::serial(pack_signature_env)]
    async fn install_rolls_back_files_when_index_commit_fails() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("rollback.zip");
        let trusted_key = write_signed_zip(
            &pack_zip,
            &[(
                "tandempack.yaml",
                "name: rollback-pack\nversion: 1.0.0\ntype: workflow\npack_id: rollback-pack\n",
            )],
        );
        let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
        let pack_root = root.join("packs");
        std::fs::create_dir_all(pack_root.join(INDEX_FILE))
            .expect("make index path unwritable as a file");
        let manager = PackManager::new(pack_root.clone());

        let error = manager
            .install(PackInstallRequest {
                path: Some(pack_zip.to_string_lossy().to_string()),
                url: None,
                expected_sha256: None,
                source: Value::Null,
            })
            .await
            .expect_err("index commit failure must abort install");

        assert!(error.to_string().contains("installation rolled back"));
        assert!(
            !pack_root.join("rollback-pack").join("1.0.0").exists(),
            "failed index persistence must not leave an installed pack directory"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[serial_test::serial(pack_signature_env)]
    async fn install_rolls_back_index_and_files_when_current_pointer_commit_fails() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("pointer-rollback.zip");
        let trusted_key = write_signed_zip(
            &pack_zip,
            &[(
                "tandempack.yaml",
                "name: pointer-rollback-pack\nversion: 1.0.0\ntype: workflow\npack_id: pointer-rollback-pack\n",
            )],
        );
        let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
        let pack_root = root.join("packs");
        std::fs::create_dir_all(pack_root.join("pointer-rollback-pack").join(CURRENT_FILE))
            .expect("make current pointer path a directory");
        let manager = PackManager::new(pack_root.clone());

        let error = manager
            .install(PackInstallRequest {
                path: Some(pack_zip.to_string_lossy().to_string()),
                url: None,
                expected_sha256: None,
                source: Value::Null,
            })
            .await
            .expect_err("current pointer failure must abort install");

        assert!(error.to_string().contains("installation rolled back"));
        assert!(manager.list().await.expect("list").is_empty());
        assert!(
            !pack_root
                .join("pointer-rollback-pack")
                .join("1.0.0")
                .exists(),
            "failed current-pointer persistence must not leave installed files"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    #[serial_test::serial(pack_signature_env)]
    async fn uninstall_rolls_back_index_and_files_when_current_pointer_commit_fails() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("uninstall-pointer-rollback.zip");
        let trusted_key = write_signed_zip(
            &pack_zip,
            &[(
                "tandempack.yaml",
                "name: uninstall-pointer-pack\nversion: 1.0.0\ntype: workflow\npack_id: uninstall-pointer-pack\n",
            )],
        );
        let _trusted_keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &trusted_key);
        let pack_root = root.join("packs");
        let manager = PackManager::new(pack_root.clone());
        let installed = manager
            .install(PackInstallRequest {
                path: Some(pack_zip.to_string_lossy().to_string()),
                url: None,
                expected_sha256: None,
                source: Value::Null,
            })
            .await
            .expect("install");
        let current = pack_root.join("uninstall-pointer-pack").join(CURRENT_FILE);
        std::fs::remove_file(&current).expect("remove current pointer");
        std::fs::create_dir(&current).expect("replace current pointer with directory");

        let error = manager
            .uninstall(PackUninstallRequest {
                pack_id: Some(installed.pack_id.clone()),
                name: None,
                version: None,
            })
            .await
            .expect_err("current pointer failure must abort uninstall");

        assert!(error.to_string().contains("uninstall rolled back"));
        assert_eq!(manager.list().await.expect("list").len(), 1);
        assert!(Path::new(&installed.install_path).exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scan_embedded_secrets_finds_real_and_ignores_examples() {
        let root = std::env::temp_dir().join(format!("tandem-pack-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let real = root.join("resources").join("token.txt");
        std::fs::create_dir_all(real.parent().expect("parent")).expect("mkdir resources");
        std::fs::write(&real, "token=ghp_example_not_real_but_pattern").expect("write real");
        let example = root.join("secrets.example.env");
        std::fs::write(&example, "API_KEY=sk-live-example").expect("write example");
        let findings = scan_embedded_secrets(&root).expect("scan");
        assert_eq!(findings.len(), 1);
        assert!(findings[0].contains("resources/token.txt"));
        let _ = std::fs::remove_dir_all(root);
    }
}
