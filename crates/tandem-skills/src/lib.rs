use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillLocation {
    Project,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillsConflictPolicy {
    Skip,
    Overwrite,
    Rename,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub location: SkillLocation,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTemplateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContent {
    pub info: SkillInfo,
    pub content: String,
    pub base_dir: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsImportPreviewItem {
    pub source: String,
    pub valid: bool,
    pub name: Option<String>,
    pub description: Option<String>,
    pub conflict: bool,
    pub action: String,
    pub target_path: Option<String>,
    pub error: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub requires: Vec<String>,
    pub compatibility: Option<String>,
    pub triggers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsImportPreview {
    pub items: Vec<SkillsImportPreviewItem>,
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub conflicts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsImportResult {
    pub imported: Vec<SkillInfo>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCatalogRecord {
    pub info: SkillInfo,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub missing_sections: Vec<String>,
    #[serde(default)]
    pub schedule_compatibility: Vec<String>,
    #[serde(default)]
    pub has_manifest: bool,
    #[serde(default)]
    pub has_workflow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillValidationIssue {
    pub code: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillValidationItem {
    pub source: String,
    pub valid: bool,
    pub name: Option<String>,
    #[serde(default)]
    pub issues: Vec<SkillValidationIssue>,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub missing_sections: Vec<String>,
    #[serde(default)]
    pub schedule_compatibility: Vec<String>,
    #[serde(default)]
    pub has_manifest: bool,
    #[serde(default)]
    pub has_workflow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillValidationReport {
    pub items: Vec<SkillValidationItem>,
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRouterMatch {
    pub skill_name: String,
    pub confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRouterResult {
    pub decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,
    pub confidence: f64,
    pub reason: String,
    #[serde(default)]
    pub top_matches: Vec<SkillRouterMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillBundleArtifacts {
    pub skill_md: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_yaml: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automation_example_yaml: Option<String>,
}

#[derive(Debug, Clone)]
struct SkillFrontmatter {
    name: String,
    description: String,
    version: Option<String>,
    author: Option<String>,
    tags: Vec<String>,
    requires: Vec<String>,
    compatibility: Option<String>,
    triggers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatterYaml {
    name: String,
    description: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    requires: Option<Vec<String>>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    triggers: Option<Vec<String>>,
    #[serde(default)]
    metadata: Option<HashMap<String, String>>,
    #[serde(default)]
    license: Option<String>,
}

#[derive(Debug, Clone)]
struct SkillCandidate {
    source: String,
    content: String,
}

#[derive(Debug, Clone)]
pub struct SkillService {
    workspace_root: Option<PathBuf>,
    global_write_root: PathBuf,
    global_discovery_roots: Vec<PathBuf>,
    template_roots: Vec<PathBuf>,
}

impl SkillService {
    pub fn for_workspace(workspace_root: Option<PathBuf>) -> Self {
        let global_write_root = default_global_write_root();
        let global_discovery_roots = default_global_discovery_roots(&global_write_root);
        let template_roots = default_template_roots();
        Self {
            workspace_root,
            global_write_root,
            global_discovery_roots,
            template_roots,
        }
    }

    pub fn with_roots(
        workspace_root: Option<PathBuf>,
        global_write_root: PathBuf,
        template_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            workspace_root,
            global_discovery_roots: vec![global_write_root.clone()],
            global_write_root,
            template_roots,
        }
    }

    pub fn with_discovery_roots(
        workspace_root: Option<PathBuf>,
        global_write_root: PathBuf,
        global_discovery_roots: Vec<PathBuf>,
        template_roots: Vec<PathBuf>,
    ) -> Self {
        Self {
            workspace_root,
            global_write_root,
            global_discovery_roots,
            template_roots,
        }
    }

    pub fn list_skills(&self) -> Result<Vec<SkillInfo>, String> {
        let mut out = Vec::new();
        let mut seen_names = HashSet::new();
        for (root, location) in self.skill_roots() {
            if !root.exists() || !root.is_dir() {
                continue;
            }
            let entries =
                fs::read_dir(&root).map_err(|e| format!("Failed to read {:?}: {}", root, e))?;
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_dir() {
                    continue;
                }
                let skill_file = entry.path().join("SKILL.md");
                if !skill_file.exists() {
                    continue;
                }
                let content = match fs::read_to_string(&skill_file) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let parsed = match parse_skill_content_with_metadata(&content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let (name, description, _body, fm) = parsed;
                let dedupe_key = name.to_ascii_lowercase();
                if seen_names.contains(&dedupe_key) {
                    continue;
                }
                seen_names.insert(dedupe_key);
                out.push(SkillInfo {
                    name,
                    description,
                    location: location.clone(),
                    path: entry.path().to_string_lossy().to_string(),
                    version: fm.version,
                    author: fm.author,
                    tags: fm.tags,
                    requires: fm.requires,
                    compatibility: fm.compatibility,
                    triggers: fm.triggers,
                    parse_error: None,
                });
            }
        }
        out.sort_by(|a, b| {
            let loc_a = match a.location {
                SkillLocation::Project => 0,
                SkillLocation::Global => 1,
            };
            let loc_b = match b.location {
                SkillLocation::Project => 0,
                SkillLocation::Global => 1,
            };
            loc_a.cmp(&loc_b).then(a.name.cmp(&b.name))
        });
        Ok(out)
    }

    pub fn list_catalog(&self) -> Result<Vec<SkillCatalogRecord>, String> {
        let list = self.list_skills()?;
        let mut out = Vec::new();
        for info in list {
            let skill_dir = PathBuf::from(&info.path);
            let skill_path = skill_dir.join("SKILL.md");
            let content = match fs::read_to_string(&skill_path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let parsed = match parse_skill_content_with_metadata(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let (_name, _description, body, _fm) = parsed;
            let analysis = analyze_skill_markdown(&body);
            out.push(SkillCatalogRecord {
                info,
                sections: analysis.sections,
                missing_sections: analysis.missing_sections,
                schedule_compatibility: analysis.schedule_compatibility,
                has_manifest: skill_dir.join("skill.yaml").exists()
                    || skill_dir.join("manifest.yaml").exists(),
                has_workflow: skill_dir.join("workflow.yaml").exists(),
            });
        }
        Ok(out)
    }

    pub fn load_skill(&self, name: &str) -> Result<Option<SkillContent>, String> {
        let target = name.trim();
        if target.is_empty() {
            return Ok(None);
        }
        for (root, location) in self.skill_roots() {
            let skill_dir = root.join(target);
            let skill_file = skill_dir.join("SKILL.md");
            if !skill_file.exists() {
                continue;
            }
            let content = fs::read_to_string(&skill_file)
                .map_err(|e| format!("Failed to read {:?}: {}", skill_file, e))?;
            let (parsed_name, description, _body, fm) =
                parse_skill_content_with_metadata(&content)?;
            let files = sample_files(&skill_dir, 10);
            let info = SkillInfo {
                name: parsed_name,
                description,
                location,
                path: skill_dir.to_string_lossy().to_string(),
                version: fm.version,
                author: fm.author,
                tags: fm.tags,
                requires: fm.requires,
                compatibility: fm.compatibility,
                triggers: fm.triggers,
                parse_error: None,
            };
            return Ok(Some(SkillContent {
                info,
                content,
                base_dir: skill_dir.to_string_lossy().to_string(),
                files,
            }));
        }
        Ok(None)
    }

    pub fn import_skill_from_content(
        &self,
        content: &str,
        location: SkillLocation,
    ) -> Result<SkillInfo, String> {
        let (name, description, _body, fm) = parse_skill_content_with_metadata(content)?;
        let target_dir = self.base_dir_for(location.clone(), None)?.join(&name);
        fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Failed to create {:?}: {}", target_dir, e))?;
        fs::write(target_dir.join("SKILL.md"), content)
            .map_err(|e| format!("Failed to write {:?}: {}", target_dir, e))?;
        Ok(SkillInfo {
            name,
            description,
            location,
            path: target_dir.to_string_lossy().to_string(),
            version: fm.version,
            author: fm.author,
            tags: fm.tags,
            requires: fm.requires,
            compatibility: fm.compatibility,
            triggers: fm.triggers,
            parse_error: None,
        })
    }

    pub fn skills_import_preview(
        &self,
        file_or_path: &str,
        location: SkillLocation,
        namespace: Option<String>,
        conflict_policy: SkillsConflictPolicy,
    ) -> Result<SkillsImportPreview, String> {
        let namespace = normalize_namespace(namespace);
        let base_dir = self.base_dir_for(location, namespace.as_deref())?;
        let candidates = load_skill_candidates(file_or_path)?;
        let mut items = Vec::new();
        let mut valid = 0usize;
        let mut invalid = 0usize;
        let mut conflicts = 0usize;

        for c in candidates {
            match parse_skill_content_with_metadata(&c.content) {
                Ok((name, description, _body, fm)) => {
                    let conflict = base_dir.join(&name).exists();
                    if conflict {
                        conflicts += 1;
                    }
                    let final_name =
                        if conflict && matches!(conflict_policy, SkillsConflictPolicy::Rename) {
                            resolve_conflict_name(&base_dir, &name)
                        } else {
                            name.clone()
                        };
                    let action = if !conflict {
                        "create".to_string()
                    } else {
                        match conflict_policy {
                            SkillsConflictPolicy::Skip => "skip".to_string(),
                            SkillsConflictPolicy::Overwrite => "overwrite".to_string(),
                            SkillsConflictPolicy::Rename => "rename".to_string(),
                        }
                    };
                    items.push(SkillsImportPreviewItem {
                        source: c.source,
                        valid: true,
                        name: Some(final_name.clone()),
                        description: Some(description),
                        conflict,
                        action,
                        target_path: Some(base_dir.join(&final_name).to_string_lossy().to_string()),
                        error: None,
                        version: fm.version,
                        author: fm.author,
                        tags: fm.tags,
                        requires: fm.requires,
                        compatibility: fm.compatibility,
                        triggers: fm.triggers,
                    });
                    valid += 1;
                }
                Err(e) => {
                    items.push(SkillsImportPreviewItem {
                        source: c.source,
                        valid: false,
                        name: None,
                        description: None,
                        conflict: false,
                        action: "invalid".to_string(),
                        target_path: None,
                        error: Some(e),
                        version: None,
                        author: None,
                        tags: Vec::new(),
                        requires: Vec::new(),
                        compatibility: None,
                        triggers: Vec::new(),
                    });
                    invalid += 1;
                }
            }
        }

        Ok(SkillsImportPreview {
            total: items.len(),
            valid,
            invalid,
            conflicts,
            items,
        })
    }

    pub fn skills_import(
        &self,
        file_or_path: &str,
        location: SkillLocation,
        namespace: Option<String>,
        conflict_policy: SkillsConflictPolicy,
    ) -> Result<SkillsImportResult, String> {
        let namespace = normalize_namespace(namespace);
        let base_dir = self.base_dir_for(location.clone(), namespace.as_deref())?;
        fs::create_dir_all(&base_dir)
            .map_err(|e| format!("Failed to create {:?}: {}", base_dir, e))?;
        let candidates = load_skill_candidates(file_or_path)?;

        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        let mut errors = Vec::new();

        for c in candidates {
            let parsed = parse_skill_content_with_metadata(&c.content);
            let (name, description, _body, fm) = match parsed {
                Ok(v) => v,
                Err(e) => {
                    errors.push(format!("{}: {}", c.source, e));
                    continue;
                }
            };
            let existing = base_dir.join(&name);
            let final_name = if existing.exists() {
                match conflict_policy {
                    SkillsConflictPolicy::Skip => {
                        skipped.push(name.clone());
                        continue;
                    }
                    SkillsConflictPolicy::Overwrite => name.clone(),
                    SkillsConflictPolicy::Rename => resolve_conflict_name(&base_dir, &name),
                }
            } else {
                name.clone()
            };
            let target_dir = base_dir.join(&final_name);
            if target_dir.exists() {
                fs::remove_dir_all(&target_dir)
                    .map_err(|e| format!("Failed to remove {:?}: {}", target_dir, e))?;
            }
            fs::create_dir_all(&target_dir)
                .map_err(|e| format!("Failed to create {:?}: {}", target_dir, e))?;
            fs::write(target_dir.join("SKILL.md"), &c.content)
                .map_err(|e| format!("Failed to write {:?}: {}", target_dir, e))?;
            imported.push(SkillInfo {
                name: final_name,
                description,
                location: location.clone(),
                path: target_dir.to_string_lossy().to_string(),
                version: fm.version,
                author: fm.author,
                tags: fm.tags,
                requires: fm.requires,
                compatibility: fm.compatibility,
                triggers: fm.triggers,
                parse_error: None,
            });
        }

        Ok(SkillsImportResult {
            imported,
            skipped,
            errors,
        })
    }

    pub fn delete_skill(&self, name: &str, location: SkillLocation) -> Result<bool, String> {
        let target = self.base_dir_for(location, None)?.join(name);
        if !target.exists() {
            return Ok(false);
        }
        fs::remove_dir_all(&target).map_err(|e| format!("Failed to remove {:?}: {}", target, e))?;
        Ok(true)
    }

    pub fn list_templates(&self) -> Result<Vec<SkillTemplateInfo>, String> {
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for root in self.template_roots.iter().filter(|p| p.exists()) {
            let entries = match fs::read_dir(root) {
                Ok(v) => v,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_dir() {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().to_string();
                if seen.contains(&id) {
                    continue;
                }
                let skill_file = entry.path().join("SKILL.md");
                if !skill_file.exists() {
                    continue;
                }
                let content = match fs::read_to_string(&skill_file) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let (name, description, _body, fm) =
                    match parse_skill_content_with_metadata(&content) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                seen.insert(id.clone());
                out.push(SkillTemplateInfo {
                    id,
                    name,
                    description,
                    requires: fm.requires,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn install_template(
        &self,
        template_id: &str,
        location: SkillLocation,
    ) -> Result<SkillInfo, String> {
        let template_dir = self
            .find_template_dir(template_id)
            .ok_or_else(|| format!("Template '{}' not found", template_id))?;
        let content = fs::read_to_string(template_dir.join("SKILL.md"))
            .map_err(|e| format!("Failed to read template '{}': {}", template_id, e))?;
        let (name, description, _body, fm) = parse_skill_content_with_metadata(&content)?;

        let target_dir = self.base_dir_for(location.clone(), None)?.join(&name);
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir)
                .map_err(|e| format!("Failed to remove {:?}: {}", target_dir, e))?;
        }
        copy_dir_recursive(&template_dir, &target_dir)?;

        Ok(SkillInfo {
            name,
            description,
            location,
            path: target_dir.to_string_lossy().to_string(),
            version: fm.version,
            author: fm.author,
            tags: fm.tags,
            requires: fm.requires,
            compatibility: fm.compatibility,
            triggers: fm.triggers,
            parse_error: None,
        })
    }

    pub fn validate_skill_source(
        &self,
        content: Option<&str>,
        file_or_path: Option<&str>,
    ) -> Result<SkillValidationReport, String> {
        let candidates = if let Some(path_or_inline) = file_or_path {
            load_skill_candidates(path_or_inline)?
        } else if let Some(raw) = content {
            load_skill_candidates(raw)?
        } else {
            return Err("Missing content or file_or_path for validation".to_string());
        };
        let mut items = Vec::new();
        let mut valid_count = 0usize;
        let mut invalid_count = 0usize;

        for candidate in candidates {
            let mut issues = Vec::<SkillValidationIssue>::new();
            let mut sections = Vec::<String>::new();
            let mut missing_sections = Vec::<String>::new();
            let mut schedule_compatibility = Vec::<String>::new();
            let mut name = None;
            let mut valid = true;
            let (has_manifest, has_workflow) = companion_flags_from_candidate(&candidate.source);

            match parse_skill_content_with_metadata(&candidate.content) {
                Ok((parsed_name, _description, body, _fm)) => {
                    name = Some(parsed_name);
                    let analysis = analyze_skill_markdown(&body);
                    sections = analysis.sections;
                    missing_sections = analysis.missing_sections;
                    schedule_compatibility = analysis.schedule_compatibility;
                    if !missing_sections.is_empty() {
                        valid = false;
                        issues.push(SkillValidationIssue {
                            code: "missing_required_sections".to_string(),
                            level: "error".to_string(),
                            message: format!(
                                "Missing required sections: {}",
                                missing_sections.join(", ")
                            ),
                        });
                    }
                    if schedule_compatibility.is_empty() {
                        issues.push(SkillValidationIssue {
                            code: "missing_schedule_compatibility_values".to_string(),
                            level: "warning".to_string(),
                            message: "Schedule compatibility section is present but no values were parsed."
                                .to_string(),
                        });
                    }
                }
                Err(error) => {
                    valid = false;
                    issues.push(SkillValidationIssue {
                        code: "invalid_skill_frontmatter".to_string(),
                        level: "error".to_string(),
                        message: error,
                    });
                }
            }
            if valid {
                valid_count += 1;
            } else {
                invalid_count += 1;
            }
            items.push(SkillValidationItem {
                source: candidate.source,
                valid,
                name,
                issues,
                sections,
                missing_sections,
                schedule_compatibility,
                has_manifest,
                has_workflow,
            });
        }

        Ok(SkillValidationReport {
            total: items.len(),
            valid: valid_count,
            invalid: invalid_count,
            items,
        })
    }

    pub fn route_skill_match(
        &self,
        goal: &str,
        max_matches: usize,
        threshold: f64,
    ) -> Result<SkillRouterResult, String> {
        let normalized_goal = normalize_text(goal);
        if normalized_goal.is_empty() {
            return Ok(SkillRouterResult {
                decision: "no_match".to_string(),
                skill_name: None,
                confidence: 0.0,
                reason: "goal is empty".to_string(),
                top_matches: Vec::new(),
            });
        }

        let catalog = self.list_catalog()?;
        let mut scored = Vec::<SkillRouterMatch>::new();
        for row in catalog {
            let mut best = 0.0f64;
            let mut reason = "name/description overlap".to_string();

            for trigger in &row.info.triggers {
                let normalized_trigger = normalize_text(trigger);
                if normalized_trigger.is_empty() {
                    continue;
                }
                if normalized_goal.contains(&normalized_trigger) {
                    best = best.max(0.98);
                    reason = format!("goal contains trigger '{}'", trigger);
                    continue;
                }
                let overlap = token_overlap_score(&normalized_goal, &normalized_trigger);
                if overlap > best {
                    best = overlap;
                    reason = format!("trigger overlap '{}'", trigger);
                }
            }

            let fallback = format!("{} {}", row.info.name, row.info.description);
            let fallback_score = token_overlap_score(&normalized_goal, &normalize_text(&fallback));
            if fallback_score > best {
                best = fallback_score;
                reason = "name/description overlap".to_string();
            }

            if best > 0.0 {
                scored.push(SkillRouterMatch {
                    skill_name: row.info.name,
                    confidence: best.min(1.0),
                    reason,
                });
            }
        }

        scored.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.skill_name.cmp(&b.skill_name))
        });
        let limited = scored
            .iter()
            .take(max_matches.max(1))
            .cloned()
            .collect::<Vec<_>>();
        let top = limited.first().cloned();
        let matched = top.clone().filter(|m| m.confidence >= threshold);
        if let Some(m) = matched {
            return Ok(SkillRouterResult {
                decision: "match".to_string(),
                skill_name: Some(m.skill_name),
                confidence: m.confidence,
                reason: m.reason,
                top_matches: limited,
            });
        }

        let top_conf = top.map(|m| m.confidence).unwrap_or(0.0);
        Ok(SkillRouterResult {
            decision: "no_match".to_string(),
            skill_name: None,
            confidence: top_conf,
            reason: if limited.is_empty() {
                "no installed skills available for routing".to_string()
            } else {
                format!("top match confidence below threshold ({:.2})", threshold)
            },
            top_matches: limited,
        })
    }

    pub fn install_skill_bundle(
        &self,
        artifacts: SkillBundleArtifacts,
        location: SkillLocation,
        conflict_policy: SkillsConflictPolicy,
    ) -> Result<SkillInfo, String> {
        let (parsed_name, description, _body, fm) =
            parse_skill_content_with_metadata(&artifacts.skill_md)?;
        let base = self.base_dir_for(location.clone(), None)?;
        fs::create_dir_all(&base).map_err(|e| format!("Failed to create {:?}: {}", base, e))?;
        let existing = base.join(&parsed_name);
        let final_name = if existing.exists() {
            match conflict_policy {
                SkillsConflictPolicy::Skip => {
                    return Err(format!("Skill '{}' already exists", parsed_name));
                }
                SkillsConflictPolicy::Overwrite => parsed_name.clone(),
                SkillsConflictPolicy::Rename => resolve_conflict_name(&base, &parsed_name),
            }
        } else {
            parsed_name.clone()
        };
        let target_dir = base.join(&final_name);
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir)
                .map_err(|e| format!("Failed to remove {:?}: {}", target_dir, e))?;
        }
        fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Failed to create {:?}: {}", target_dir, e))?;
        fs::write(target_dir.join("SKILL.md"), artifacts.skill_md)
            .map_err(|e| format!("Failed to write {:?}: {}", target_dir, e))?;
        if let Some(workflow) = artifacts.workflow_yaml {
            fs::write(target_dir.join("workflow.yaml"), workflow)
                .map_err(|e| format!("Failed to write workflow.yaml: {}", e))?;
        }
        if let Some(example) = artifacts.automation_example_yaml {
            fs::write(target_dir.join("automation.example.yaml"), example)
                .map_err(|e| format!("Failed to write automation.example.yaml: {}", e))?;
        }

        Ok(SkillInfo {
            name: final_name,
            description,
            location,
            path: target_dir.to_string_lossy().to_string(),
            version: fm.version,
            author: fm.author,
            tags: fm.tags,
            requires: fm.requires,
            compatibility: fm.compatibility,
            triggers: fm.triggers,
            parse_error: None,
        })
    }

    fn skill_roots(&self) -> Vec<(PathBuf, SkillLocation)> {
        let mut roots = Vec::new();
        let mut seen = HashSet::new();
        if let Some(workspace) = &self.workspace_root {
            for candidate in [
                workspace.join(".tandem").join("skill"),
                workspace.join(".tandem").join("skills"),
            ] {
                let key = candidate.to_string_lossy().to_string();
                if seen.insert(key) {
                    roots.push((candidate, SkillLocation::Project));
                }
            }
        }
        for root in &self.global_discovery_roots {
            let key = root.to_string_lossy().to_string();
            if seen.insert(key) {
                roots.push((root.clone(), SkillLocation::Global));
            }
        }
        roots
    }

    fn base_dir_for(
        &self,
        location: SkillLocation,
        namespace: Option<&str>,
    ) -> Result<PathBuf, String> {
        let mut base = match location {
            SkillLocation::Project => self
                .workspace_root
                .as_ref()
                .ok_or_else(|| "No active workspace for project skill operation".to_string())?
                .join(".tandem")
                .join("skill"),
            SkillLocation::Global => self.global_write_root.clone(),
        };
        if let Some(ns) = namespace {
            for seg in ns.split('/') {
                if !seg.trim().is_empty() {
                    base.push(seg.trim());
                }
            }
        }
        Ok(base)
    }

    fn find_template_dir(&self, template_id: &str) -> Option<PathBuf> {
        self.template_roots
            .iter()
            .map(|r| r.join(template_id))
            .find(|p| p.exists() && p.is_dir() && p.join("SKILL.md").exists())
    }
}

fn canonical_global_skills_root() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("tandem").join("skills"))
        .or_else(|| dirs::home_dir().map(|h| h.join(".tandem").join("skills")))
        .unwrap_or_else(|| PathBuf::from("skills"))
}

fn default_global_write_root() -> PathBuf {
    canonical_global_skills_root()
}

fn default_global_discovery_roots(global_write_root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![global_write_root.to_path_buf()];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".tandem").join("skills"));
        roots.push(home.join(".agents").join("skills"));
        roots.push(home.join(".claude").join("skills"));
    }
    roots.push(canonical_global_skills_root());
    let mut dedup = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        let key = root.to_string_lossy().to_string();
        if seen.insert(key) {
            dedup.push(root);
        }
    }
    dedup
}

fn default_template_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(raw) = std::env::var("TANDEM_SKILL_TEMPLATE_DIR") {
        for item in raw.split(';') {
            let trimmed = item.trim();
            if !trimmed.is_empty() {
                out.push(PathBuf::from(trimmed));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        out.push(
            cwd.join("src-tauri")
                .join("resources")
                .join("skill-templates"),
        );
        out.push(cwd.join("resources").join("skill-templates"));
    }
    out
}

fn load_skill_candidates(file_or_path: &str) -> Result<Vec<SkillCandidate>, String> {
    let path = PathBuf::from(file_or_path);
    if path.exists() {
        if path.extension().and_then(|e| e.to_str()) == Some("zip") {
            let file = fs::File::open(&path).map_err(|e| format!("Failed to open zip: {}", e))?;
            let mut zip =
                zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {}", e))?;
            let mut out = Vec::new();
            for i in 0..zip.len() {
                let mut entry = zip
                    .by_index(i)
                    .map_err(|e| format!("Failed to read zip entry: {}", e))?;
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name().replace('\\', "/");
                if !name.to_ascii_lowercase().ends_with("skill.md") {
                    continue;
                }
                let mut content = String::new();
                entry
                    .read_to_string(&mut content)
                    .map_err(|e| format!("Non-UTF8 SKILL.md in zip entry {}: {}", name, e))?;
                out.push(SkillCandidate {
                    source: name,
                    content,
                });
            }
            if out.is_empty() {
                return Err("No SKILL.md files found in zip".to_string());
            }
            return Ok(out);
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.to_string_lossy(), e))?;
        return Ok(vec![SkillCandidate {
            source: path.to_string_lossy().to_string(),
            content,
        }]);
    }

    Ok(vec![SkillCandidate {
        source: "inline".to_string(),
        content: file_or_path.to_string(),
    }])
}

fn resolve_conflict_name(base: &Path, name: &str) -> String {
    if !base.join(name).exists() {
        return name.to_string();
    }
    for i in 2..=10_000 {
        let candidate = format!("{}-{}", name, i);
        if !base.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{}-copy", name)
}

fn normalize_namespace(namespace: Option<String>) -> Option<String> {
    namespace.and_then(|ns| {
        let clean = ns.trim().replace('\\', "/");
        let parts: Vec<String> = clean
            .split('/')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty() && *p != "." && *p != "..")
            .map(|p| {
                p.chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect::<String>()
            })
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("/"))
        }
    })
}

fn parse_skill_content_with_metadata(
    content: &str,
) -> Result<(String, String, String, SkillFrontmatter), String> {
    let (frontmatter, body) = split_frontmatter(content)?;
    validate_skill_name(&frontmatter.name)?;
    Ok((
        frontmatter.name.clone(),
        frontmatter.description.clone(),
        body,
        frontmatter,
    ))
}

#[derive(Debug, Default)]
struct SkillMarkdownAnalysis {
    sections: Vec<String>,
    missing_sections: Vec<String>,
    schedule_compatibility: Vec<String>,
}

fn analyze_skill_markdown(body: &str) -> SkillMarkdownAnalysis {
    let sections_map = parse_markdown_sections(body);
    let mut sections = sections_map
        .keys()
        .map(|v| canonical_section_name(v))
        .collect::<Vec<_>>();
    sections.sort();
    sections.dedup();
    let required = required_section_names();
    let missing_sections = required
        .iter()
        .filter(|req| !sections_map.contains_key(&req.to_ascii_lowercase()))
        .cloned()
        .collect::<Vec<_>>();
    let schedule_compatibility = sections_map
        .get("schedule compatibility")
        .map(|lines| parse_schedule_compatibility(lines))
        .unwrap_or_default();
    SkillMarkdownAnalysis {
        sections,
        missing_sections,
        schedule_compatibility,
    }
}

fn required_section_names() -> Vec<String> {
    vec![
        "Purpose".to_string(),
        "Inputs".to_string(),
        "Agents".to_string(),
        "Tools".to_string(),
        "Workflow".to_string(),
        "Outputs".to_string(),
        "Schedule compatibility".to_string(),
    ]
}

fn canonical_section_name(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "purpose" => "Purpose".to_string(),
        "inputs" => "Inputs".to_string(),
        "agents" => "Agents".to_string(),
        "tools" => "Tools".to_string(),
        "workflow" => "Workflow".to_string(),
        "outputs" => "Outputs".to_string(),
        "schedule compatibility" => "Schedule compatibility".to_string(),
        other => {
            let mut chars = other.chars();
            if let Some(first) = chars.next() {
                let mut out = first.to_ascii_uppercase().to_string();
                out.push_str(chars.as_str());
                out
            } else {
                String::new()
            }
        }
    }
}

fn parse_markdown_sections(body: &str) -> HashMap<String, Vec<String>> {
    let mut sections = HashMap::<String, Vec<String>>::new();
    let mut current = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let heading = trimmed.trim_start_matches('#').trim().to_ascii_lowercase();
            if !heading.is_empty() {
                current = heading.clone();
                sections.entry(heading).or_default();
            }
            continue;
        }
        if current.is_empty() {
            continue;
        }
        sections
            .entry(current.clone())
            .or_default()
            .push(trimmed.to_string());
    }
    sections
}

fn parse_schedule_compatibility(lines: &[String]) -> Vec<String> {
    let allowed = ["manual", "cron", "interval"];
    let mut found = Vec::<String>::new();
    for line in lines {
        let cleaned = line
            .trim()
            .trim_start_matches('-')
            .trim_start_matches('*')
            .trim()
            .to_ascii_lowercase();
        if cleaned.is_empty() {
            continue;
        }
        for item in cleaned.split(',').map(str::trim) {
            if allowed.contains(&item) && !found.iter().any(|v| v == item) {
                found.push(item.to_string());
            }
        }
    }
    found
}

fn companion_flags_from_candidate(source: &str) -> (bool, bool) {
    let path = PathBuf::from(source);
    if !path.exists() || path.extension().and_then(|v| v.to_str()) == Some("zip") {
        return (false, false);
    }
    let Some(parent) = path.parent() else {
        return (false, false);
    };
    (
        parent.join("skill.yaml").exists() || parent.join("manifest.yaml").exists(),
        parent.join("workflow.yaml").exists(),
    )
}

fn normalize_text(input: &str) -> String {
    input
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn token_overlap_score(a: &str, b: &str) -> f64 {
    let a_tokens = a.split_whitespace().collect::<HashSet<_>>();
    let b_tokens = b.split_whitespace().collect::<HashSet<_>>();
    if a_tokens.is_empty() || b_tokens.is_empty() {
        return 0.0;
    }
    let common = a_tokens.intersection(&b_tokens).count() as f64;
    common / (b_tokens.len() as f64)
}

fn split_frontmatter(content: &str) -> Result<(SkillFrontmatter, String), String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut start = None;
    let mut end = None;
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            if start.is_none() {
                start = Some(i);
            } else if end.is_none() {
                end = Some(i);
                break;
            }
        }
    }
    let (start, end) = match (start, end) {
        (Some(s), Some(e)) if s < e => (s, e),
        _ => return Err("Invalid SKILL.md format: missing frontmatter".to_string()),
    };
    let yaml = lines[start + 1..end].join("\n");
    let parsed: SkillFrontmatterYaml =
        serde_yaml::from_str(&yaml).map_err(|e| format!("Failed to parse frontmatter: {}", e))?;
    let _ = parsed.metadata.as_ref().map(|m| m.len());
    let _ = parsed.license.as_ref().map(|s| s.len());
    let frontmatter = SkillFrontmatter {
        name: parsed.name,
        description: parsed.description,
        version: parsed.version,
        author: parsed.author,
        tags: parsed.tags.unwrap_or_default(),
        requires: parsed.requires.unwrap_or_default(),
        compatibility: parsed.compatibility,
        triggers: parsed.triggers.unwrap_or_default(),
    };
    let body = if end + 1 < lines.len() {
        lines[end + 1..].join("\n")
    } else {
        String::new()
    };
    Ok((frontmatter, body))
}

fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Skill name must be 1-64 characters".to_string());
    }
    let chars: Vec<char> = name.chars().collect();
    if chars.first() == Some(&'-') || chars.last() == Some(&'-') {
        return Err("Skill name cannot start or end with a hyphen".to_string());
    }
    let mut prev_hyphen = false;
    for c in chars {
        if c == '-' {
            if prev_hyphen {
                return Err("Skill name cannot contain consecutive hyphens".to_string());
            }
            prev_hyphen = true;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            prev_hyphen = false;
        } else {
            return Err("Skill name must be lowercase alphanumeric with hyphens only".to_string());
        }
    }
    Ok(())
}

fn sample_files(root: &Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let walker = walkdir::WalkDir::new(root).follow_links(false).into_iter();
    for entry in walker.filter_map(Result::ok) {
        if out.len() >= limit {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|v| v.to_str()) == Some("SKILL.md") {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().to_string());
        }
    }
    out
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("Failed to create {:?}: {}", dst, e))?;
    let entries = fs::read_dir(src).map_err(|e| format!("Failed to read {:?}: {}", src, e))?;
    for entry_res in entries {
        let entry = entry_res.map_err(|e| format!("Failed to read entry: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let ft = entry
            .file_type()
            .map_err(|e| format!("Failed to stat {:?}: {}", src_path, e))?;
        if ft.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ft.is_file() {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("Failed to copy {:?}: {}", src_path, e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_skill(name: &str, description: &str) -> String {
        format!(
            r#"---
name: {}
description: {}
---

# {}

workflow
"#,
            name, description, name
        )
    }

    fn sample_skill_full(name: &str, description: &str) -> String {
        format!(
            r#"---
name: {}
description: {}
triggers:
  - check my email every morning
---

# Skill: {}

## Purpose
Summarize important information.

## Inputs
- inbox

## Agents
- reader

## Tools
- webfetch

## Workflow
1. fetch
2. summarize

## Outputs
- daily summary

## Schedule compatibility
- cron
- manual
"#,
            name, description, name
        )
    }

    #[test]
    fn list_and_load_from_project_and_global() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let global = tmp.path().join("global").join("skills");
        fs::create_dir_all(workspace.join(".tandem").join("skill").join("proj-skill"))
            .expect("mkdir");
        fs::create_dir_all(global.join("global-skill")).expect("mkdir");
        fs::write(
            workspace
                .join(".tandem")
                .join("skill")
                .join("proj-skill")
                .join("SKILL.md"),
            sample_skill("proj-skill", "project"),
        )
        .expect("write");
        fs::write(
            global.join("global-skill").join("SKILL.md"),
            sample_skill("global-skill", "global"),
        )
        .expect("write");

        let svc = SkillService::with_roots(Some(workspace), global, vec![]);
        let list = svc.list_skills().expect("list");
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|s| s.name == "proj-skill"));
        assert!(list.iter().any(|s| s.name == "global-skill"));

        let loaded = svc.load_skill("proj-skill").expect("load").expect("exists");
        assert!(loaded.content.contains("workflow"));
    }

    #[test]
    fn import_preview_and_conflicts() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let project_root = workspace.join(".tandem").join("skill");
        fs::create_dir_all(project_root.join("dup-skill")).expect("mkdir");
        fs::write(
            project_root.join("dup-skill").join("SKILL.md"),
            sample_skill("dup-skill", "old"),
        )
        .expect("write");
        let svc = SkillService::with_roots(
            Some(workspace),
            tmp.path().join("global").join("skills"),
            vec![],
        );
        let preview = svc
            .skills_import_preview(
                &sample_skill("dup-skill", "new"),
                SkillLocation::Project,
                None,
                SkillsConflictPolicy::Rename,
            )
            .expect("preview");
        assert_eq!(preview.total, 1);
        assert_eq!(preview.conflicts, 1);
        assert_eq!(preview.items[0].action, "rename");
    }

    #[test]
    fn install_template_copies_extra_files() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let templates = tmp.path().join("templates");
        fs::create_dir_all(
            templates
                .join("product-marketing-context")
                .join("references"),
        )
        .expect("mkdir");
        fs::write(
            templates.join("product-marketing-context").join("SKILL.md"),
            sample_skill("product-marketing-context", "desc"),
        )
        .expect("write");
        fs::write(
            templates
                .join("product-marketing-context")
                .join("references")
                .join("product-marketing-context-template.md"),
            "template",
        )
        .expect("write");
        let svc = SkillService::with_roots(
            Some(workspace.clone()),
            tmp.path().join("global").join("skills"),
            vec![templates],
        );
        let installed = svc
            .install_template("product-marketing-context", SkillLocation::Project)
            .expect("install");
        assert_eq!(installed.name, "product-marketing-context");
        assert!(workspace
            .join(".tandem")
            .join("skill")
            .join("product-marketing-context")
            .join("references")
            .join("product-marketing-context-template.md")
            .exists());
    }

    #[test]
    fn discovery_dedupes_by_priority_project_over_global() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let project_root = workspace.join(".tandem").join("skill");
        let global_root = tmp.path().join("home").join(".tandem").join("skills");
        fs::create_dir_all(project_root.join("dup-skill")).expect("mkdir");
        fs::create_dir_all(global_root.join("dup-skill")).expect("mkdir");
        fs::write(
            project_root.join("dup-skill").join("SKILL.md"),
            sample_skill("dup-skill", "project version"),
        )
        .expect("write");
        fs::write(
            global_root.join("dup-skill").join("SKILL.md"),
            sample_skill("dup-skill", "global version"),
        )
        .expect("write");

        let svc = SkillService::with_discovery_roots(
            Some(workspace),
            global_root.clone(),
            vec![global_root],
            vec![],
        );
        let list = svc.list_skills().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].location, SkillLocation::Project);
        assert_eq!(list[0].description, "project version");
    }

    #[test]
    fn discovery_scans_external_ecosystem_roots() {
        let tmp = TempDir::new().expect("tempdir");
        let home = tmp.path().join("home");
        let tandem_root = home.join(".tandem").join("skills");
        let agents_root = home.join(".agents").join("skills");
        let claude_root = home.join(".claude").join("skills");
        fs::create_dir_all(tandem_root.join("tandem-skill")).expect("mkdir");
        fs::create_dir_all(agents_root.join("agents-skill")).expect("mkdir");
        fs::create_dir_all(claude_root.join("claude-skill")).expect("mkdir");
        fs::write(
            tandem_root.join("tandem-skill").join("SKILL.md"),
            sample_skill("tandem-skill", "tandem"),
        )
        .expect("write");
        fs::write(
            agents_root.join("agents-skill").join("SKILL.md"),
            sample_skill("agents-skill", "agents"),
        )
        .expect("write");
        fs::write(
            claude_root.join("claude-skill").join("SKILL.md"),
            sample_skill("claude-skill", "claude"),
        )
        .expect("write");

        let svc = SkillService::with_discovery_roots(
            None,
            tandem_root.clone(),
            vec![tandem_root, agents_root, claude_root],
            vec![],
        );
        let names = svc
            .list_skills()
            .expect("list")
            .into_iter()
            .map(|s| s.name)
            .collect::<Vec<_>>();
        assert!(names.iter().any(|n| n == "tandem-skill"));
        assert!(names.iter().any(|n| n == "agents-skill"));
        assert!(names.iter().any(|n| n == "claude-skill"));
    }

    #[test]
    fn validate_skill_checks_required_sections() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let svc = SkillService::with_roots(
            Some(workspace),
            tmp.path().join("global").join("skills"),
            vec![],
        );
        let report = svc
            .validate_skill_source(Some(&sample_skill("incomplete", "desc")), None)
            .expect("validate");
        assert_eq!(report.total, 1);
        assert_eq!(report.invalid, 1);
        assert_eq!(report.items[0].valid, false);
        assert!(report.items[0]
            .issues
            .iter()
            .any(|i| i.code == "missing_required_sections"));
    }

    #[test]
    fn catalog_reports_sections_and_workflow_flags() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let skill_dir = workspace.join(".tandem").join("skill").join("email-digest");
        fs::create_dir_all(&skill_dir).expect("mkdir");
        fs::write(
            skill_dir.join("SKILL.md"),
            sample_skill_full("email-digest", "digest emails"),
        )
        .expect("write");
        fs::write(skill_dir.join("workflow.yaml"), "kind: pack_builder_recipe").expect("write");
        let svc = SkillService::with_roots(
            Some(workspace),
            tmp.path().join("global").join("skills"),
            vec![],
        );
        let catalog = svc.list_catalog().expect("catalog");
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].info.name, "email-digest");
        assert_eq!(catalog[0].has_workflow, true);
        assert!(catalog[0]
            .schedule_compatibility
            .iter()
            .any(|v| v == "cron"));
    }

    #[test]
    fn router_matches_trigger_phrase() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let skill_dir = workspace.join(".tandem").join("skill").join("email-digest");
        fs::create_dir_all(&skill_dir).expect("mkdir");
        fs::write(
            skill_dir.join("SKILL.md"),
            sample_skill_full("email-digest", "digest emails"),
        )
        .expect("write");
        let svc = SkillService::with_roots(
            Some(workspace),
            tmp.path().join("global").join("skills"),
            vec![],
        );
        let matched = svc
            .route_skill_match("Check my email every morning and summarize it", 3, 0.35)
            .expect("match");
        assert_eq!(matched.decision, "match");
        assert_eq!(matched.skill_name.as_deref(), Some("email-digest"));
        assert!(matched.confidence >= 0.35);
    }

    #[test]
    fn install_skill_bundle_writes_expected_files() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let svc = SkillService::with_roots(
            Some(workspace.clone()),
            tmp.path().join("global").join("skills"),
            vec![],
        );
        let artifacts = SkillBundleArtifacts {
            skill_md: sample_skill_full("bundle-skill", "bundle"),
            workflow_yaml: Some("kind: pack_builder_recipe\nskill_id: bundle-skill\n".to_string()),
            automation_example_yaml: Some("name: Example\nschedule:\n  type: manual\n".to_string()),
        };
        let installed = svc
            .install_skill_bundle(
                artifacts,
                SkillLocation::Project,
                SkillsConflictPolicy::Overwrite,
            )
            .expect("install");
        assert_eq!(installed.name, "bundle-skill");
        let dir = workspace.join(".tandem").join("skill").join("bundle-skill");
        assert!(dir.join("SKILL.md").exists());
        assert!(dir.join("workflow.yaml").exists());
        assert!(dir.join("automation.example.yaml").exists());
    }

    #[test]
    fn install_skill_bundle_respects_rename_conflict_policy() {
        let tmp = TempDir::new().expect("tempdir");
        let workspace = tmp.path().join("workspace");
        let base = workspace.join(".tandem").join("skill").join("bundle-skill");
        fs::create_dir_all(&base).expect("mkdir");
        fs::write(
            base.join("SKILL.md"),
            sample_skill_full("bundle-skill", "old"),
        )
        .expect("write");
        let svc = SkillService::with_roots(
            Some(workspace),
            tmp.path().join("global").join("skills"),
            vec![],
        );
        let artifacts = SkillBundleArtifacts {
            skill_md: sample_skill_full("bundle-skill", "new"),
            workflow_yaml: None,
            automation_example_yaml: None,
        };
        let installed = svc
            .install_skill_bundle(
                artifacts,
                SkillLocation::Project,
                SkillsConflictPolicy::Rename,
            )
            .expect("install");
        assert_ne!(installed.name, "bundle-skill");
        assert!(installed.name.starts_with("bundle-skill-"));
    }
}
