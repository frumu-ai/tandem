use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillTemplateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub requires: Vec<String>,
}

fn resolve_templates_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource directory: {}", e))?;

    let candidates = vec![
        // Production bundles (we include `resources/**` in tauri.conf.json).
        resource_dir.join("resources").join("skill-templates"),
        resource_dir.join("skill-templates"),
    ];

    let templates_dir = candidates
        .iter()
        .find(|p| p.exists())
        .cloned()
        .or_else(|| {
            #[cfg(debug_assertions)]
            {
                let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("resources")
                    .join("skill-templates");
                if dev.exists() {
                    return Some(dev);
                }
            }
            None
        })
        .ok_or_else(|| {
            format!(
                "Skill templates directory not found. Looked in: {:?}",
                candidates
            )
        })?;

    Ok(templates_dir)
}

pub fn list_skill_templates(app: &AppHandle) -> Result<Vec<SkillTemplateInfo>, String> {
    let templates_dir = resolve_templates_dir(app)?;
    let entries = fs::read_dir(&templates_dir)
        .map_err(|e| format!("Failed to read {:?}: {}", templates_dir, e))?;

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let id = entry.file_name().to_string_lossy().to_string();
        let skill_file = entry.path().join("SKILL.md");
        if !skill_file.exists() {
            continue;
        }

        let content = fs::read_to_string(&skill_file)
            .map_err(|e| format!("Failed to read {:?}: {}", skill_file, e))?;

        let (name, description, requires) = match crate::skills::parse_skill_frontmatter(&content) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Skipping invalid skill template {:?}: {}", skill_file, e);
                continue;
            }
        };

        out.push(SkillTemplateInfo {
            id,
            name,
            description,
            requires,
        });
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

pub fn read_skill_template_content(app: &AppHandle, template_id: &str) -> Result<String, String> {
    let templates_dir = resolve_templates_dir(app)?;
    let skill_file = templates_dir.join(template_id).join("SKILL.md");
    if !skill_file.exists() {
        return Err(format!("Skill template not found: {}", template_id));
    }

    fs::read_to_string(&skill_file).map_err(|e| format!("Failed to read {:?}: {}", skill_file, e))
}
