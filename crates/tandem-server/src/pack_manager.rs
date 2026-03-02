use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{copy, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
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
    pub capabilities: Value,
    #[serde(default)]
    pub entrypoints: Value,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInstallRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
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
        let manifest_path = PathBuf::from(&installed.install_path).join(MARKER_FILE);
        let manifest_raw = tokio::fs::read_to_string(&manifest_path)
            .await
            .with_context(|| format!("read {}", manifest_path.display()))?;
        let manifest: Value = serde_yaml::from_str(&manifest_raw).context("parse manifest yaml")?;
        let trust = serde_json::json!({
            "publisher_verification": "unknown",
            "signature": "unsigned",
        });
        let risk = serde_json::json!({
            "routines_enabled": installed.routines_enabled,
            "non_portable_dependencies": false,
        });
        Ok(PackInspection {
            installed,
            manifest,
            trust,
            risk,
        })
    }

    pub async fn install(&self, input: PackInstallRequest) -> anyhow::Result<PackInstallRecord> {
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
        validate_manifest(&manifest)?;
        let sha256 = sha256_file(&source_file)?;
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

        let install_parent = self.root.join(&manifest.name);
        let install_target = install_parent.join(&manifest.version);
        if install_target.exists() {
            let _ = tokio::fs::remove_dir_all(&stage_root).await;
            return Err(anyhow!(
                "pack already installed: {}@{}",
                manifest.name,
                manifest.version
            ));
        }
        tokio::fs::create_dir_all(&install_parent).await?;
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

        tokio::fs::write(
            install_parent.join(CURRENT_FILE),
            format!("{}\n", manifest.version),
        )
        .await
        .ok();

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
                    "url": input.url
                })
            } else {
                input.source
            },
            marker_detected: true,
            routines_enabled: false,
        };
        self.write_record(record.clone()).await?;
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

        let mut index = self.read_index().await?;
        let Some(record) = select_record(&index, selector, req.version.as_deref()) else {
            return Err(anyhow!("pack not found"));
        };
        let install_path = PathBuf::from(&record.install_path);
        if install_path.exists() {
            tokio::fs::remove_dir_all(&install_path).await.ok();
        }
        index.packs.retain(|row| {
            !(row.pack_id == record.pack_id
                && row.name == record.name
                && row.version == record.version
                && row.install_path == record.install_path)
        });
        self.write_index(&index).await?;
        self.repoint_current_if_needed(&record.name).await?;
        Ok(record)
    }

    pub async fn export(&self, req: PackExportRequest) -> anyhow::Result<PackExportResult> {
        let index = self.read_index().await?;
        let selector = req.pack_id.as_deref().or(req.name.as_deref());
        let Some(record) = select_record(&index, selector, req.version.as_deref()) else {
            return Err(anyhow!("pack not found"));
        };
        let pack_dir = PathBuf::from(&record.install_path);
        if !pack_dir.exists() {
            return Err(anyhow!("installed pack path missing"));
        }
        let output = if let Some(path) = req.output_path {
            PathBuf::from(path)
        } else {
            self.root
                .join(EXPORTS_DIR)
                .join(format!("{}-{}.zip", record.name, record.version))
        };
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        zip_directory(&pack_dir, &output)?;
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
        let response = reqwest::get(url)
            .await
            .with_context(|| format!("download {}", url))?;
        let bytes = response.bytes().await.context("read body")?;
        if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
            return Err(anyhow!(
                "downloaded archive exceeds max size ({} > {})",
                bytes.len(),
                MAX_ARCHIVE_BYTES
            ));
        }
        tokio::fs::write(&stage, &bytes).await?;
        Ok(stage)
    }

    async fn ensure_layout(&self) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.root).await?;
        tokio::fs::create_dir_all(self.root.join(STAGING_DIR)).await?;
        tokio::fs::create_dir_all(self.root.join(EXPORTS_DIR)).await?;
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

    async fn write_record(&self, record: PackInstallRecord) -> anyhow::Result<()> {
        let _index_guard = self.index_lock.lock().await;
        let mut index = self.read_index_unlocked().await?;
        index.packs.retain(|row| {
            !(row.pack_id == record.pack_id
                && row.name == record.name
                && row.version == record.version)
        });
        index.packs.push(record);
        self.write_index_unlocked(&index).await
    }

    async fn repoint_current_if_needed(&self, pack_name: &str) -> anyhow::Result<()> {
        let index = self.read_index().await?;
        let mut versions = index
            .packs
            .iter()
            .filter(|row| row.name == pack_name)
            .collect::<Vec<_>>();
        versions.sort_by(|a, b| b.installed_at_ms.cmp(&a.installed_at_ms));
        let current_path = self.root.join(pack_name).join(CURRENT_FILE);
        if let Some(latest) = versions.first() {
            tokio::fs::write(current_path, format!("{}\n", latest.version))
                .await
                .ok();
        } else if current_path.exists() {
            tokio::fs::remove_file(current_path).await.ok();
        }
        Ok(())
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

fn validate_manifest(manifest: &PackManifest) -> anyhow::Result<()> {
    if manifest.name.trim().is_empty() {
        return Err(anyhow!("manifest.name is required"));
    }
    if manifest.version.trim().is_empty() {
        return Err(anyhow!("manifest.version is required"));
    }
    if manifest.pack_type.trim().is_empty() {
        return Err(anyhow!("manifest.type is required"));
    }
    Ok(())
}

fn safe_extract_zip(zip_path: &Path, out_dir: &Path) -> anyhow::Result<()> {
    let file = File::open(zip_path).with_context(|| format!("open {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).context("open zip archive")?;
    let mut extracted_files = 0usize;
    let mut extracted_total = 0u64;
    let mut compressed_total = 0u64;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).context("zip entry read")?;
        let entry_name = entry.name().to_string();
        if entry_name.ends_with('/') {
            continue;
        }
        validate_zip_entry_name(&entry_name)?;
        let out_path = out_dir.join(&entry_name);
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

fn zip_directory(src_dir: &Path, output_zip: &Path) -> anyhow::Result<()> {
    let file =
        File::create(output_zip).with_context(|| format!("create {}", output_zip.display()))?;
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
            let rel = path
                .strip_prefix(src_dir)
                .context("strip source prefix")?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
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
}
