use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const BUILTINS_DIR: &str = "presets/builtins";
const OVERRIDES_DIR: &str = "presets/overrides";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetRecord {
    pub id: String,
    pub version: String,
    pub kind: String,
    pub layer: String,
    #[serde(default)]
    pub pack: Option<String>,
    pub path: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresetIndex {
    #[serde(default)]
    pub skill_modules: Vec<PresetRecord>,
    #[serde(default)]
    pub agent_presets: Vec<PresetRecord>,
    #[serde(default)]
    pub automation_presets: Vec<PresetRecord>,
    #[serde(default)]
    pub pack_presets: Vec<PresetRecord>,
    pub generated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetExportResult {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone)]
pub struct PresetRegistry {
    packs_root: PathBuf,
    runtime_root: PathBuf,
}

impl PresetRegistry {
    pub fn new(packs_root: PathBuf, runtime_root: PathBuf) -> Self {
        Self {
            packs_root,
            runtime_root,
        }
    }

    pub async fn index(&self) -> anyhow::Result<PresetIndex> {
        let mut out = PresetIndex {
            generated_at_ms: crate::now_ms(),
            ..PresetIndex::default()
        };
        self.index_builtin_and_overrides(&mut out)?;
        self.index_installed_packs(&mut out)?;
        sort_records(&mut out.skill_modules);
        sort_records(&mut out.agent_presets);
        sort_records(&mut out.automation_presets);
        sort_records(&mut out.pack_presets);
        Ok(out)
    }

    pub async fn fork_to_override(
        &self,
        kind: &str,
        source_path: &Path,
        target_id: Option<&str>,
    ) -> anyhow::Result<PathBuf> {
        let source = std::fs::canonicalize(source_path)
            .with_context(|| format!("canonicalize {}", source_path.display()))?;
        let packs_root = self
            .packs_root
            .canonicalize()
            .unwrap_or_else(|_| self.packs_root.clone());
        let runtime_root = self
            .runtime_root
            .canonicalize()
            .unwrap_or_else(|_| self.runtime_root.clone());
        if !source.starts_with(&packs_root) && !source.starts_with(&runtime_root) {
            return Err(anyhow::anyhow!(
                "fork source path must be inside packs/runtime roots"
            ));
        }
        let content = std::fs::read_to_string(&source)
            .with_context(|| format!("read {}", source.display()))?;
        let id = target_id
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .or_else(|| {
                source
                    .file_stem()
                    .map(|v| v.to_string_lossy().to_string())
                    .filter(|v| !v.trim().is_empty())
            })
            .unwrap_or_else(|| "override".to_string());
        self.save_override(kind, &id, &content).await
    }

    pub async fn save_override(
        &self,
        kind: &str,
        id: &str,
        content: &str,
    ) -> anyhow::Result<PathBuf> {
        let dir = self
            .runtime_root
            .join(OVERRIDES_DIR)
            .join(kind_dir_name(kind)?);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(format!("{id}.yaml"));
        tokio::fs::write(&path, content).await?;
        Ok(path)
    }

    pub async fn delete_override(&self, kind: &str, id: &str) -> anyhow::Result<bool> {
        let path = self
            .runtime_root
            .join(OVERRIDES_DIR)
            .join(kind_dir_name(kind)?)
            .join(format!("{id}.yaml"));
        if path.exists() {
            tokio::fs::remove_file(&path).await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn export_overrides(
        &self,
        name: &str,
        version: &str,
        output_path: Option<&str>,
    ) -> anyhow::Result<PresetExportResult> {
        let name = name.trim();
        let version = version.trim();
        if name.is_empty() || version.is_empty() {
            return Err(anyhow::anyhow!("name and version are required"));
        }
        let overrides_root = self.runtime_root.join(OVERRIDES_DIR);
        let has_any = [
            "skill_modules",
            "agent_presets",
            "automation_presets",
            "pack_presets",
        ]
        .iter()
        .any(|dir| overrides_root.join(dir).exists());
        if !has_any {
            return Err(anyhow::anyhow!("no overrides found to export"));
        }

        let stage_root = self
            .runtime_root
            .join("presets")
            .join(".export-staging")
            .join(format!("export-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&stage_root).await?;
        let stage_pack = stage_root.join("pack");
        tokio::fs::create_dir_all(&stage_pack).await?;

        for dir in [
            "skill_modules",
            "agent_presets",
            "automation_presets",
            "pack_presets",
        ] {
            let src = overrides_root.join(dir);
            if src.exists() {
                copy_dir_recursive(&src, &stage_pack.join(dir))?;
            }
        }
        let manifest = format!(
            "name: {name}\nversion: {version}\ntype: bundle\npack_id: {name}\nmanifest_schema_version: v1\nentrypoints: {{}}\ncapabilities:\n  required: []\n  optional: []\ncontents: {{}}\n"
        );
        tokio::fs::write(stage_pack.join("tandempack.yaml"), manifest).await?;
        tokio::fs::write(
            stage_pack.join("README.md"),
            "# Exported Preset Overrides\n\nGenerated by Tandem preset registry export.\n",
        )
        .await?;

        let output = if let Some(path) = output_path {
            PathBuf::from(path)
        } else {
            self.runtime_root
                .join("exports")
                .join(format!("{name}-{version}.zip"))
        };
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        zip_directory(&stage_pack, &output)?;
        let bytes = tokio::fs::metadata(&output).await?.len();
        let sha256 = sha256_file(&output)?;
        let _ = tokio::fs::remove_dir_all(stage_root).await;
        Ok(PresetExportResult {
            path: output.to_string_lossy().to_string(),
            sha256,
            bytes,
        })
    }

    fn index_builtin_and_overrides(&self, out: &mut PresetIndex) -> anyhow::Result<()> {
        let builtins = self.runtime_root.join(BUILTINS_DIR);
        self.index_layer_dir(&builtins, "builtin", None, out)?;
        let overrides = self.runtime_root.join(OVERRIDES_DIR);
        self.index_layer_dir(&overrides, "override", None, out)?;
        Ok(())
    }

    fn index_installed_packs(&self, out: &mut PresetIndex) -> anyhow::Result<()> {
        if !self.packs_root.exists() {
            return Ok(());
        }
        let entries = std::fs::read_dir(&self.packs_root)
            .with_context(|| format!("read {}", self.packs_root.display()))?;
        for entry in entries {
            let entry = entry?;
            let pack_name = entry.file_name().to_string_lossy().to_string();
            if pack_name.starts_with('.') || pack_name == "exports" || pack_name == "bindings" {
                continue;
            }
            let pack_dir = entry.path();
            if !pack_dir.is_dir() {
                continue;
            }
            for ver_entry in std::fs::read_dir(&pack_dir)? {
                let ver_entry = ver_entry?;
                let ver_name = ver_entry.file_name().to_string_lossy().to_string();
                if ver_name == "current" {
                    continue;
                }
                let ver_dir = ver_entry.path();
                if !ver_dir.is_dir() {
                    continue;
                }
                self.index_layer_dir(
                    &ver_dir,
                    "pack",
                    Some(format!("{pack_name}@{ver_name}")),
                    out,
                )?;
            }
        }
        Ok(())
    }

    fn index_layer_dir(
        &self,
        base: &Path,
        layer: &str,
        pack: Option<String>,
        out: &mut PresetIndex,
    ) -> anyhow::Result<()> {
        collect_presets_into(
            &base.join("skill_modules"),
            "skill_module",
            layer,
            pack.clone(),
            &mut out.skill_modules,
        )?;
        collect_presets_into(
            &base.join("agent_presets"),
            "agent_preset",
            layer,
            pack.clone(),
            &mut out.agent_presets,
        )?;
        collect_presets_into(
            &base.join("automation_presets"),
            "automation_preset",
            layer,
            pack.clone(),
            &mut out.automation_presets,
        )?;
        collect_presets_into(
            &base.join("pack_presets"),
            "pack_preset",
            layer,
            pack,
            &mut out.pack_presets,
        )?;
        Ok(())
    }
}

fn sort_records(items: &mut [PresetRecord]) {
    items.sort_by(|a, b| {
        a.layer
            .cmp(&b.layer)
            .then_with(|| a.pack.cmp(&b.pack))
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.version.cmp(&b.version))
    });
}

fn collect_presets_into(
    dir: &Path,
    kind: &str,
    layer: &str,
    pack: Option<String>,
    out: &mut Vec<PresetRecord>,
) -> anyhow::Result<()> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .map(|v| v.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "yaml" && ext != "yml" && ext != "json" {
            continue;
        }
        let meta = read_preset_metadata(&path)?;
        out.push(PresetRecord {
            id: meta.id,
            version: meta.version,
            kind: kind.to_string(),
            layer: layer.to_string(),
            pack: pack.clone(),
            path: path.to_string_lossy().to_string(),
            tags: meta.tags,
            publisher: meta.publisher,
            required_capabilities: meta.required_capabilities,
        });
    }
    Ok(())
}

fn kind_dir_name(kind: &str) -> anyhow::Result<&'static str> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "skill_module" | "skill_modules" => Ok("skill_modules"),
        "agent_preset" | "agent_presets" => Ok("agent_presets"),
        "automation_preset" | "automation_presets" => Ok("automation_presets"),
        "pack_preset" | "pack_presets" => Ok("pack_presets"),
        other => Err(anyhow::anyhow!("unsupported preset kind: {}", other)),
    }
}

#[derive(Debug, Clone, Default)]
struct PresetMetadata {
    id: String,
    version: String,
    tags: Vec<String>,
    publisher: Option<String>,
    required_capabilities: Vec<String>,
}

fn read_preset_metadata(path: &Path) -> anyhow::Result<PresetMetadata> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let ext = path
        .extension()
        .map(|v| v.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let value: Value = if ext == "json" {
        serde_json::from_str(&raw).unwrap_or(Value::Null)
    } else {
        serde_yaml::from_str(&raw).unwrap_or(Value::Null)
    };
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            path.file_stem()
                .map(|v| v.to_string_lossy().to_string())
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or_else(|| "unknown".to_string());
    let version = value
        .get("version")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "0.0.0".to_string());
    let tags = value
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let publisher = value
        .get("publisher")
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
            if let Some(obj) = v.as_object() {
                return obj
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        obj.get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    });
            }
            None
        })
        .filter(|v| !v.trim().is_empty());
    let required_capabilities = value
        .pointer("/capabilities/required")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(PresetMetadata {
        id,
        version,
        tags,
        publisher,
        required_capabilities,
    })
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if entry.file_type()?.is_file() {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copy {} -> {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    Ok(())
}

fn zip_directory(src_dir: &Path, output_zip: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::create(output_zip)
        .with_context(|| format!("create {}", output_zip.display()))?;
    let mut writer = ZipWriter::new(file);
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut stack = vec![src_dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let mut entries = std::fs::read_dir(&current)?
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
            let mut input = std::fs::File::open(&path)?;
            writer.start_file(rel, opts)?;
            std::io::copy(&mut input, &mut writer)?;
        }
    }
    writer.finish()?;
    Ok(())
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn indexes_layered_sources() {
        let root = std::env::temp_dir().join(format!("tandem-presets-test-{}", Uuid::new_v4()));
        let packs_root = root.join("packs");
        let runtime_root = root.join("runtime");
        std::fs::create_dir_all(runtime_root.join("presets/builtins/skill_modules"))
            .expect("mkdir builtins");
        std::fs::create_dir_all(runtime_root.join("presets/overrides/agent_presets"))
            .expect("mkdir overrides");
        std::fs::create_dir_all(packs_root.join("sample-pack/1.0.0/automation_presets"))
            .expect("mkdir packs");
        std::fs::create_dir_all(runtime_root.join("presets/overrides/pack_presets"))
            .expect("mkdir pack presets");
        std::fs::write(
            runtime_root.join("presets/builtins/skill_modules/git.yaml"),
            "id: git.core\nversion: 1.0.0\ntags: [git]\npublisher: tandem\ncapabilities:\n  required:\n    - github.create_pull_request\n",
        )
        .expect("write");
        std::fs::write(
            runtime_root.join("presets/overrides/agent_presets/dev.yaml"),
            "id: agent.dev\nversion: 1.1.0\n",
        )
        .expect("write");
        std::fs::write(
            packs_root.join("sample-pack/1.0.0/automation_presets/release.yaml"),
            "id: auto.release\nversion: 2.0.0\n",
        )
        .expect("write");
        std::fs::write(
            runtime_root.join("presets/overrides/pack_presets/news_pack.yaml"),
            "id: pack.news\nversion: 0.4.1\n",
        )
        .expect("write");

        let registry = PresetRegistry::new(packs_root, runtime_root);
        let index = registry.index().await.expect("index");
        assert_eq!(index.skill_modules.len(), 1);
        assert_eq!(index.agent_presets.len(), 1);
        assert_eq!(index.automation_presets.len(), 1);
        assert_eq!(index.pack_presets.len(), 1);
        assert_eq!(index.skill_modules[0].layer, "builtin");
        assert_eq!(index.agent_presets[0].layer, "override");
        assert_eq!(index.automation_presets[0].layer, "pack");
        assert_eq!(index.pack_presets[0].kind, "pack_preset");
        assert_eq!(
            index.automation_presets[0].pack.as_deref(),
            Some("sample-pack@1.0.0")
        );
        assert_eq!(index.skill_modules[0].publisher.as_deref(), Some("tandem"));
        assert_eq!(
            index.skill_modules[0].required_capabilities,
            vec!["github.create_pull_request".to_string()]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn save_and_delete_override_roundtrip() {
        let root = std::env::temp_dir().join(format!("tandem-presets-test-{}", Uuid::new_v4()));
        let registry = PresetRegistry::new(root.join("packs"), root.join("runtime"));
        let path = registry
            .save_override(
                "agent_preset",
                "dev-agent",
                "id: dev-agent\nversion: 1.0.1\n",
            )
            .await
            .expect("save");
        assert!(path.exists());
        let deleted = registry
            .delete_override("agent_preset", "dev-agent")
            .await
            .expect("delete");
        assert!(deleted);
        assert!(!path.exists());
        let pack_path = registry
            .save_override(
                "pack_preset",
                "generated-pack",
                "id: generated-pack\nversion: 0.4.1\n",
            )
            .await
            .expect("save pack preset");
        assert!(pack_path.exists());
        let deleted_pack = registry
            .delete_override("pack_preset", "generated-pack")
            .await
            .expect("delete pack preset");
        assert!(deleted_pack);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn export_overrides_writes_zip_bundle() {
        let root = std::env::temp_dir().join(format!("tandem-presets-test-{}", Uuid::new_v4()));
        let registry = PresetRegistry::new(root.join("packs"), root.join("runtime"));
        registry
            .save_override("skill_module", "git-core", "id: git-core\nversion: 1.0.0\n")
            .await
            .expect("save");
        let exported = registry
            .export_overrides("team-presets", "1.0.0", None)
            .await
            .expect("export");
        assert!(PathBuf::from(&exported.path).exists());
        assert!(exported.bytes > 0);
        assert!(!exported.sha256.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
