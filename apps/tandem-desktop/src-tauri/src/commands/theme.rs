// Theme / Appearance
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomBackgroundFit {
    Cover,
    Contain,
    Tile,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomBackgroundSettings {
    pub enabled: bool,
    pub file_name: Option<String>,
    pub fit: CustomBackgroundFit,
    /// 0.0 - 1.0
    pub opacity: f32,
}

impl Default for CustomBackgroundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            file_name: None,
            fit: CustomBackgroundFit::Cover,
            opacity: 0.30,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CustomBackgroundInfo {
    pub settings: CustomBackgroundSettings,
    pub file_path: Option<String>,
}

const CUSTOM_BG_STORE_KEY: &str = "custom_background";
const CUSTOM_BG_DIR_NAME: &str = "backgrounds";
const CUSTOM_BG_FILE_STEM: &str = "custom-background";
const CUSTOM_BG_MAX_BYTES: u64 = 20 * 1024 * 1024; // 20MB

fn custom_bg_dir(app: &AppHandle) -> Result<PathBuf> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| TandemError::IoError(format!("Failed to get app data dir: {}", e)))?;
    Ok(app_data_dir.join(CUSTOM_BG_DIR_NAME))
}

fn is_allowed_custom_bg_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "webp")
}

fn custom_bg_file_name_for_ext(ext: &str) -> String {
    format!("{}.{}", CUSTOM_BG_FILE_STEM, ext)
}

fn clear_existing_custom_bg_files(dir: &PathBuf) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.starts_with(&format!("{}.", CUSTOM_BG_FILE_STEM)) {
                let _ = fs::remove_file(&path);
            }
        }
    }

    Ok(())
}

fn resolve_custom_bg_file_path(
    app: &AppHandle,
    file_name: &Option<String>,
) -> Result<Option<String>> {
    let Some(file_name) = file_name else {
        return Ok(None);
    };

    let dir = custom_bg_dir(app)?;
    let path = dir.join(file_name);
    if path.exists() {
        Ok(Some(path.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

fn read_custom_bg_settings(app: &AppHandle) -> CustomBackgroundSettings {
    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get(CUSTOM_BG_STORE_KEY) {
            if let Ok(settings) = serde_json::from_value::<CustomBackgroundSettings>(value.clone())
            {
                return settings;
            }
        }
    }

    CustomBackgroundSettings::default()
}

fn write_custom_bg_settings(app: &AppHandle, settings: &CustomBackgroundSettings) -> Result<()> {
    if let Ok(store) = app.store("settings.json") {
        store.set(CUSTOM_BG_STORE_KEY, serde_json::to_value(settings)?);
        let _ = store.save();
    }
    Ok(())
}

/// Get the user's selected theme id
#[tauri::command]
pub fn get_user_theme(app: AppHandle) -> Result<String> {
    // Default to the new design-system theme
    let default_theme = "charcoal_fire".to_string();

    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get("user_theme") {
            if let Some(theme_id) = value.as_str() {
                return Ok(theme_id.to_string());
            }
        }
    }

    Ok(default_theme)
}

/// Persist the user's selected theme id
#[tauri::command]
pub fn set_user_theme(app: AppHandle, theme_id: String) -> Result<()> {
    if let Ok(store) = app.store("settings.json") {
        store.set("user_theme", serde_json::json!(theme_id));
        let _ = store.save();
    }
    Ok(())
}

/// Get the user's custom background configuration (and resolved path to the stored image, if any)
#[tauri::command]
pub fn get_custom_background(app: AppHandle) -> Result<CustomBackgroundInfo> {
    let mut settings = read_custom_bg_settings(&app);
    let file_path = resolve_custom_bg_file_path(&app, &settings.file_name)?;

    // Heal invalid state: if the file is missing, disable the feature.
    if settings.enabled && settings.file_name.is_some() && file_path.is_none() {
        settings.enabled = false;
        settings.file_name = None;
        let _ = write_custom_bg_settings(&app, &settings);
    }

    Ok(CustomBackgroundInfo {
        settings,
        file_path,
    })
}

/// Set the custom background image by copying from an existing path into AppData.
#[tauri::command]
pub fn set_custom_background_image(
    app: AppHandle,
    source_path: String,
) -> Result<CustomBackgroundInfo> {
    let src = PathBuf::from(&source_path);
    if !src.exists() || !src.is_file() {
        return Err(TandemError::NotFound(format!(
            "Image not found: {}",
            source_path
        )));
    }

    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| TandemError::ValidationError("File has no extension".to_string()))?;

    if !is_allowed_custom_bg_ext(&ext) {
        return Err(TandemError::ValidationError(format!(
            "Unsupported image type: .{} (allowed: png, jpg, jpeg, webp)",
            ext
        )));
    }

    let meta = fs::metadata(&src)?;
    if meta.len() > CUSTOM_BG_MAX_BYTES {
        return Err(TandemError::ValidationError(format!(
            "Image is too large (max 20MB): {} bytes",
            meta.len()
        )));
    }

    let dir = custom_bg_dir(&app)?;
    fs::create_dir_all(&dir)?;
    clear_existing_custom_bg_files(&dir)?;

    let file_name = custom_bg_file_name_for_ext(&ext);
    let dest = dir.join(&file_name);
    fs::copy(&src, &dest)?;

    let mut settings = read_custom_bg_settings(&app);
    settings.enabled = true;
    settings.file_name = Some(file_name);
    // Keep any existing fit/opacity preferences.
    write_custom_bg_settings(&app, &settings)?;

    Ok(CustomBackgroundInfo {
        settings,
        file_path: Some(dest.to_string_lossy().to_string()),
    })
}

/// Set the custom background image by writing bytes into AppData (used for drag/drop).
#[tauri::command]
pub fn set_custom_background_image_bytes(
    app: AppHandle,
    file_name: String,
    bytes: Vec<u8>,
) -> Result<CustomBackgroundInfo> {
    if bytes.len() as u64 > CUSTOM_BG_MAX_BYTES {
        return Err(TandemError::ValidationError(format!(
            "Image is too large (max 20MB): {} bytes",
            bytes.len()
        )));
    }

    let ext = PathBuf::from(&file_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| TandemError::ValidationError("File has no extension".to_string()))?;

    if !is_allowed_custom_bg_ext(&ext) {
        return Err(TandemError::ValidationError(format!(
            "Unsupported image type: .{} (allowed: png, jpg, jpeg, webp)",
            ext
        )));
    }

    let dir = custom_bg_dir(&app)?;
    fs::create_dir_all(&dir)?;
    clear_existing_custom_bg_files(&dir)?;

    let stored_file_name = custom_bg_file_name_for_ext(&ext);
    let dest = dir.join(&stored_file_name);
    fs::write(&dest, bytes)?;

    let mut settings = read_custom_bg_settings(&app);
    settings.enabled = true;
    settings.file_name = Some(stored_file_name);
    write_custom_bg_settings(&app, &settings)?;

    Ok(CustomBackgroundInfo {
        settings,
        file_path: Some(dest.to_string_lossy().to_string()),
    })
}

/// Update custom background settings (fit/opacity/enabled). Does not change the stored image.
#[tauri::command]
pub fn set_custom_background_settings(
    app: AppHandle,
    settings: CustomBackgroundSettings,
) -> Result<()> {
    if !(0.0..=1.0).contains(&settings.opacity) {
        return Err(TandemError::ValidationError(
            "Opacity must be between 0.0 and 1.0".to_string(),
        ));
    }

    write_custom_bg_settings(&app, &settings)?;
    Ok(())
}

/// Clear the stored custom background image and disable the feature.
#[tauri::command]
pub fn clear_custom_background_image(app: AppHandle) -> Result<()> {
    let dir = custom_bg_dir(&app)?;
    let _ = clear_existing_custom_bg_files(&dir);

    let mut settings = read_custom_bg_settings(&app);
    settings.enabled = false;
    settings.file_name = None;
    write_custom_bg_settings(&app, &settings)?;

    Ok(())
}
