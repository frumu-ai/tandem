use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresetIndex {
    #[serde(default)]
    pub skill_modules: Vec<PresetRecord>,
    #[serde(default)]
    pub agent_presets: Vec<PresetRecord>,
    #[serde(default)]
    pub automation_presets: Vec<PresetRecord>,
    pub generated_at_ms: u64,
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
            pack,
            &mut out.automation_presets,
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
        let (id, version, tags) = read_preset_metadata(&path)?;
        out.push(PresetRecord {
            id,
            version,
            kind: kind.to_string(),
            layer: layer.to_string(),
            pack: pack.clone(),
            path: path.to_string_lossy().to_string(),
            tags,
        });
    }
    Ok(())
}

fn kind_dir_name(kind: &str) -> anyhow::Result<&'static str> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "skill_module" | "skill_modules" => Ok("skill_modules"),
        "agent_preset" | "agent_presets" => Ok("agent_presets"),
        "automation_preset" | "automation_presets" => Ok("automation_presets"),
        other => Err(anyhow::anyhow!("unsupported preset kind: {}", other)),
    }
}

fn read_preset_metadata(path: &Path) -> anyhow::Result<(String, String, Vec<String>)> {
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
    Ok((id, version, tags))
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
        std::fs::write(
            runtime_root.join("presets/builtins/skill_modules/git.yaml"),
            "id: git.core\nversion: 1.0.0\ntags: [git]\n",
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

        let registry = PresetRegistry::new(packs_root, runtime_root);
        let index = registry.index().await.expect("index");
        assert_eq!(index.skill_modules.len(), 1);
        assert_eq!(index.agent_presets.len(), 1);
        assert_eq!(index.automation_presets.len(), 1);
        assert_eq!(index.skill_modules[0].layer, "builtin");
        assert_eq!(index.agent_presets[0].layer, "override");
        assert_eq!(index.automation_presets[0].layer, "pack");
        assert_eq!(
            index.automation_presets[0].pack.as_deref(),
            Some("sample-pack@1.0.0")
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
        let _ = std::fs::remove_dir_all(root);
    }
}
