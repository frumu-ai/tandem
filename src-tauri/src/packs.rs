use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize)]
pub struct PackMeta {
    pub id: String,
    pub title: String,
    pub description: String,
    pub complexity: String,
    pub time_estimate: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackInstallResult {
    pub installed_path: String,
}

pub fn list_packs() -> Vec<PackMeta> {
    vec![
        PackMeta {
            id: "micro-drama-script-studio-pack".to_string(),
            title: "Micro-Drama Script Studio".to_string(),
            description: "Create short-form scripts with structured workflows".to_string(),
            complexity: "Intermediate".to_string(),
            time_estimate: "15-20 min".to_string(),
            tags: vec!["writing".to_string(), "creative".to_string()],
        },
        PackMeta {
            id: "research-synthesis-pack".to_string(),
            title: "Research Synthesis".to_string(),
            description: "Synthesize research across multiple documents".to_string(),
            complexity: "Intermediate-Advanced".to_string(),
            time_estimate: "20-25 min".to_string(),
            tags: vec!["research".to_string(), "analysis".to_string()],
        },
        PackMeta {
            id: "web-research-refresh-pack".to_string(),
            title: "Web Research Refresh".to_string(),
            description: "Verify stale facts and refresh docs with sources".to_string(),
            complexity: "Beginner-Intermediate".to_string(),
            time_estimate: "15-20 min".to_string(),
            tags: vec!["research".to_string(), "docs".to_string()],
        },
        PackMeta {
            id: "security-playbook-pack".to_string(),
            title: "Security Playbook".to_string(),
            description: "Build a practical security runbook and checklist".to_string(),
            complexity: "Intermediate".to_string(),
            time_estimate: "20-25 min".to_string(),
            tags: vec!["security".to_string(), "compliance".to_string()],
        },
        PackMeta {
            id: "legal-research-pack".to_string(),
            title: "Legal Research".to_string(),
            description: "Analyze contracts and synthesize case notes".to_string(),
            complexity: "Intermediate-Advanced".to_string(),
            time_estimate: "20-25 min".to_string(),
            tags: vec!["legal".to_string(), "analysis".to_string()],
        },
        PackMeta {
            id: "web-starter-audit-pack".to_string(),
            title: "Web Starter Audit".to_string(),
            description: "Audit a web project for UX, a11y, and quality".to_string(),
            complexity: "Beginner-Intermediate".to_string(),
            time_estimate: "15-20 min".to_string(),
            tags: vec!["audit".to_string(), "quality".to_string()],
        },
        PackMeta {
            id: "data-visualization-pack".to_string(),
            title: "Data Visualization".to_string(),
            description: "Create publication-quality visualizations with Python".to_string(),
            complexity: "Intermediate".to_string(),
            time_estimate: "15-20 min".to_string(),
            tags: vec![
                "data".to_string(),
                "visualization".to_string(),
                "python".to_string(),
            ],
        },
        PackMeta {
            id: "finance-analysis-pack".to_string(),
            title: "Finance Analysis".to_string(),
            description: "Automate financial reporting and variance analysis".to_string(),
            complexity: "Intermediate".to_string(),
            time_estimate: "15-20 min".to_string(),
            tags: vec![
                "finance".to_string(),
                "analysis".to_string(),
                "reporting".to_string(),
            ],
        },
        PackMeta {
            id: "bio-informatics-pack".to_string(),
            title: "Bio-Informatics".to_string(),
            description: "Convert instrument data, run pipelines, and analyze single-cell data"
                .to_string(),
            complexity: "Advanced".to_string(),
            time_estimate: "30-45 min".to_string(),
            tags: vec![
                "bio".to_string(),
                "research".to_string(),
                "python".to_string(),
                "nextflow".to_string(),
            ],
        },
    ]
}

fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), String> {
    if !from.exists() {
        return Err(format!("Source does not exist: {:?}", from));
    }

    fs::create_dir_all(to).map_err(|e| format!("Failed to create directory {:?}: {}", to, e))?;

    let entries = fs::read_dir(from).map_err(|e| format!("Failed to read {:?}: {}", from, e))?;
    for entry in entries.flatten() {
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Failed to read file type {:?}: {}", entry.path(), e))?;

        let dest_path = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &dest_path).map_err(|e| {
                format!(
                    "Failed to copy file {:?} -> {:?}: {}",
                    entry.path(),
                    dest_path,
                    e
                )
            })?;
        }
    }

    Ok(())
}

fn resolve_pack_sources(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource directory: {}", e))?;

    let packs_candidates = vec![
        // Production bundles (we include `resources/**` in tauri.conf.json).
        resource_dir.join("resources").join("packs"),
        resource_dir.join("packs"),
        resource_dir
            .join("resources")
            .join("workspace-packs")
            .join("packs"),
        resource_dir.join("workspace-packs").join("packs"),
    ];

    let docs_candidates = vec![
        resource_dir.join("resources").join("pack-docs"),
        resource_dir.join("pack-docs"),
        resource_dir
            .join("resources")
            .join("workspace-packs")
            .join("pack-docs"),
        resource_dir.join("workspace-packs").join("pack-docs"),
    ];

    let packs_root = packs_candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .or_else(|| {
            #[cfg(debug_assertions)]
            {
                let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("workspace-packs")
                    .join("packs");
                if dev.exists() {
                    return Some(dev);
                }
            }
            None
        })
        .ok_or_else(|| {
            format!(
                "Pack templates not found. Looked in: {:?}",
                packs_candidates
            )
        })?;

    let pack_docs_root = docs_candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .or_else(|| {
            #[cfg(debug_assertions)]
            {
                let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join("workspace-packs")
                    .join("pack-docs");
                if dev.exists() {
                    return Some(dev);
                }
            }
            None
        })
        .ok_or_else(|| format!("Pack docs not found. Looked in: {:?}", docs_candidates))?;

    Ok((packs_root, pack_docs_root))
}

fn choose_destination_dir(destination_dir: &Path, pack_id: &str) -> Result<PathBuf, String> {
    let base_name = pack_id;
    let mut candidate = destination_dir.join(base_name);
    if !candidate.exists() {
        return Ok(candidate);
    }

    for i in 2..=100 {
        candidate = destination_dir.join(format!("{}-{}", base_name, i));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("Failed to choose a destination directory (too many conflicts)".to_string())
}

fn default_pack_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(home) = app.path().home_dir() {
        return Ok(home.join("Tandem Packs"));
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data directory: {}", e))?;
    Ok(app_data_dir.join("packs"))
}

pub fn install_pack(
    app: &AppHandle,
    pack_id: &str,
    destination_dir: &str,
) -> Result<PackInstallResult, String> {
    // Validate pack id exists (prevents path traversal and gives nicer errors).
    if !list_packs().iter().any(|p| p.id == pack_id) {
        return Err(format!("Unknown pack: {}", pack_id));
    }

    let dest_root = PathBuf::from(destination_dir);
    if !dest_root.exists() {
        fs::create_dir_all(&dest_root)
            .map_err(|e| format!("Failed to create destination {:?}: {}", dest_root, e))?;
    }
    if !dest_root.is_dir() {
        return Err(format!(
            "Destination is not a directory: {}",
            destination_dir
        ));
    }

    let (packs_root, pack_docs_root) = resolve_pack_sources(app)?;
    let source_pack_dir = packs_root.join(pack_id);
    if !source_pack_dir.exists() {
        return Err(format!(
            "Pack template not found on disk: {:?}",
            source_pack_dir
        ));
    }

    let install_dir = choose_destination_dir(&dest_root, pack_id)?;

    tracing::info!(
        "Installing pack '{}' from {:?} -> {:?}",
        pack_id,
        source_pack_dir,
        install_dir
    );

    copy_dir_recursive(&source_pack_dir, &install_dir)?;

    let start_here_source = pack_docs_root.join(pack_id).join("START_HERE.md");
    if start_here_source.exists() {
        let start_here_dest = install_dir.join("START_HERE.md");
        if let Err(e) = fs::copy(&start_here_source, &start_here_dest) {
            tracing::warn!(
                "Failed to copy START_HERE.md {:?} -> {:?}: {}",
                start_here_source,
                start_here_dest,
                e
            );
        }
    }

    let pack_info_source = pack_docs_root.join(pack_id).join("PACK_INFO.md");
    if pack_info_source.exists() {
        let pack_info_dest = install_dir.join("PACK_INFO.md");
        if let Err(e) = fs::copy(&pack_info_source, &pack_info_dest) {
            tracing::warn!(
                "Failed to copy PACK_INFO.md {:?} -> {:?}: {}",
                pack_info_source,
                pack_info_dest,
                e
            );
        }
    }

    let prompts_source = pack_docs_root.join(pack_id).join("PROMPTS.md");
    if prompts_source.exists() {
        let prompts_dest = install_dir.join("PROMPTS.md");
        if let Err(e) = fs::copy(&prompts_source, &prompts_dest) {
            tracing::warn!(
                "Failed to copy PROMPTS.md {:?} -> {:?}: {}",
                prompts_source,
                prompts_dest,
                e
            );
        }
    }

    let expected_source = pack_docs_root.join(pack_id).join("EXPECTED_OUTPUTS.md");
    if expected_source.exists() {
        let expected_dest = install_dir.join("EXPECTED_OUTPUTS.md");
        if let Err(e) = fs::copy(&expected_source, &expected_dest) {
            tracing::warn!(
                "Failed to copy EXPECTED_OUTPUTS.md {:?} -> {:?}: {}",
                expected_source,
                expected_dest,
                e
            );
        }
    }

    Ok(PackInstallResult {
        installed_path: install_dir.to_string_lossy().to_string(),
    })
}

pub fn install_pack_default(app: &AppHandle, pack_id: &str) -> Result<PackInstallResult, String> {
    let root = default_pack_root(app)?;
    install_pack(app, pack_id, &root.to_string_lossy())
}
