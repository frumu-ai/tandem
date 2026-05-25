// ============================================================================
// Python Environment (Workspace Venv Wizard)
// ============================================================================

#[tauri::command]
pub async fn python_get_status(state: State<'_, AppState>) -> Result<python_env::PythonStatus> {
    let ws = state.get_workspace_path();
    Ok(python_env::get_status(ws.as_deref()))
}

#[tauri::command]
pub async fn python_create_venv(
    state: State<'_, AppState>,
    selected: Option<String>,
) -> Result<python_env::PythonStatus> {
    let ws = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

    tokio::task::spawn_blocking(move || python_env::create_venv(&ws, selected))
        .await
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to create venv: {}", e)))?
}

#[tauri::command]
pub async fn python_install_requirements(
    state: State<'_, AppState>,
    requirements_path: String,
) -> Result<python_env::PythonInstallResult> {
    let ws = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

    let req_path = PathBuf::from(&requirements_path);
    if !state.is_path_allowed(&req_path) {
        return Err(TandemError::PermissionDenied(format!(
            "Requirements path is outside the allowed workspace: {}",
            requirements_path
        )));
    }

    tokio::task::spawn_blocking(move || python_env::install_requirements(&ws, &req_path))
        .await
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to install requirements: {}", e)))?
}

// ============================================================================
// Skills Management Commands
// ============================================================================

fn to_engine_skill_location(
    location: crate::skills::SkillLocation,
) -> tandem_skills::SkillLocation {
    match location {
        crate::skills::SkillLocation::Project => tandem_skills::SkillLocation::Project,
        crate::skills::SkillLocation::Global => tandem_skills::SkillLocation::Global,
    }
}

fn from_engine_skill_location(
    location: tandem_skills::SkillLocation,
) -> crate::skills::SkillLocation {
    match location {
        tandem_skills::SkillLocation::Project => crate::skills::SkillLocation::Project,
        tandem_skills::SkillLocation::Global => crate::skills::SkillLocation::Global,
    }
}

fn from_engine_skill_info(info: tandem_skills::SkillInfo) -> crate::skills::SkillInfo {
    crate::skills::SkillInfo {
        name: info.name,
        description: info.description,
        location: from_engine_skill_location(info.location),
        path: info.path,
        version: info.version,
        author: info.author,
        tags: info.tags,
        requires: info.requires,
        compatibility: info.compatibility,
        triggers: info.triggers,
        parse_error: info.parse_error,
    }
}

fn conflict_policy_text(policy: &SkillsConflictPolicy) -> String {
    match policy {
        SkillsConflictPolicy::Skip => "skip".to_string(),
        SkillsConflictPolicy::Overwrite => "overwrite".to_string(),
        SkillsConflictPolicy::Rename => "rename".to_string(),
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillsConflictPolicy {
    Skip,
    Overwrite,
    Rename,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SkillsImportPreview {
    pub items: Vec<SkillsImportPreviewItem>,
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub conflicts: usize,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SkillsImportResult {
    pub imported: Vec<crate::skills::SkillInfo>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn skills_import_preview(
    state: State<'_, AppState>,
    file_or_path: String,
    location: crate::skills::SkillLocation,
    namespace: Option<String>,
    conflict_policy: SkillsConflictPolicy,
) -> Result<SkillsImportPreview> {
    let preview = state
        .sidecar
        .skills_import_preview(
            file_or_path,
            to_engine_skill_location(location),
            namespace,
            conflict_policy_text(&conflict_policy),
        )
        .await?;
    Ok(SkillsImportPreview {
        items: preview
            .items
            .into_iter()
            .map(|item| SkillsImportPreviewItem {
                source: item.source,
                valid: item.valid,
                name: item.name,
                description: item.description,
                conflict: item.conflict,
                action: item.action,
                target_path: item.target_path,
                error: item.error,
                version: item.version,
                author: item.author,
                tags: item.tags,
                requires: item.requires,
                compatibility: item.compatibility,
                triggers: item.triggers,
            })
            .collect(),
        total: preview.total,
        valid: preview.valid,
        invalid: preview.invalid,
        conflicts: preview.conflicts,
    })
}

#[tauri::command]
pub async fn skills_import(
    state: State<'_, AppState>,
    file_or_path: String,
    location: crate::skills::SkillLocation,
    namespace: Option<String>,
    conflict_policy: SkillsConflictPolicy,
) -> Result<SkillsImportResult> {
    let result = state
        .sidecar
        .skills_import(
            file_or_path,
            to_engine_skill_location(location),
            namespace,
            conflict_policy_text(&conflict_policy),
        )
        .await?;
    Ok(SkillsImportResult {
        imported: result
            .imported
            .into_iter()
            .map(from_engine_skill_info)
            .collect(),
        skipped: result.skipped,
        errors: result.errors,
    })
}

/// List all installed skills
#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<crate::skills::SkillInfo>> {
    let skills = state.sidecar.list_skills().await?;
    Ok(skills.into_iter().map(from_engine_skill_info).collect())
}

/// Import a skill from raw SKILL.md content
#[tauri::command]
pub async fn import_skill(
    state: State<'_, AppState>,
    content: String,
    location: crate::skills::SkillLocation,
) -> Result<crate::skills::SkillInfo> {
    let skill = state
        .sidecar
        .import_skill_content(content, to_engine_skill_location(location))
        .await?;
    Ok(from_engine_skill_info(skill))
}

/// Delete a skill
#[tauri::command]
pub async fn delete_skill(
    state: State<'_, AppState>,
    name: String,
    location: crate::skills::SkillLocation,
) -> Result<()> {
    state
        .sidecar
        .delete_skill(name, to_engine_skill_location(location))
        .await
}

// ============================================================================
// Starter Skill Templates (offline)
// ============================================================================

#[tauri::command]
pub async fn skills_list_templates(
    state: State<'_, AppState>,
    _app: AppHandle,
) -> Result<Vec<crate::skill_templates::SkillTemplateInfo>> {
    let templates = state.sidecar.list_skill_templates().await?;
    Ok(templates
        .into_iter()
        .map(|t| crate::skill_templates::SkillTemplateInfo {
            id: t.id,
            name: t.name,
            description: t.description,
            requires: t.requires,
        })
        .collect())
}

#[tauri::command]
pub async fn skills_install_template(
    state: State<'_, AppState>,
    _app: AppHandle,
    template_id: String,
    location: crate::skills::SkillLocation,
) -> Result<crate::skills::SkillInfo> {
    let installed = state
        .sidecar
        .install_skill_template(template_id, to_engine_skill_location(location))
        .await?;
    Ok(from_engine_skill_info(installed))
}
