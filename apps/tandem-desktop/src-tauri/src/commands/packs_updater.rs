// ============================================================================
// Packs (guided workflows)
// ============================================================================

#[tauri::command]
pub fn packs_list() -> Vec<crate::packs::PackMeta> {
    crate::packs::list_packs()
}

#[tauri::command]
pub fn packs_install(
    app: AppHandle,
    pack_id: String,
    destination_dir: String,
) -> Result<crate::packs::PackInstallResult> {
    crate::packs::install_pack(&app, &pack_id, &destination_dir).map_err(TandemError::InvalidConfig)
}

#[tauri::command]
pub fn packs_install_default(
    app: AppHandle,
    state: State<'_, AppState>,
    pack_id: String,
) -> Result<crate::packs::PackInstallResult> {
    if let Some(workspace_path) = state.get_workspace_path() {
        // Prefer agent-templates (renamed from workspace-packs); fall back for
        // existing workspaces that still have the old directory name.
        let base = workspace_path;
        let preferred = base.join("agent-templates");
        let legacy = base.join("workspace-packs");
        let install_root = if preferred.exists() || !legacy.exists() {
            preferred
        } else {
            legacy
        };
        return crate::packs::install_pack(&app, &pack_id, &install_root.to_string_lossy())
            .map_err(TandemError::InvalidConfig);
    }
    crate::packs::install_pack_default(&app, &pack_id).map_err(TandemError::InvalidConfig)
}

// ============================================================================
// Updater helpers
// ============================================================================

/// Returns an updater target override when we can reliably detect packaging.
///
/// Why: On Linux, `@tauri-apps/plugin-updater` defaults to `linux-x86_64`, which
/// in our `latest.json` maps to the AppImage. If the app is installed via a
/// `.deb` (e.g. `/usr/bin/tandem`), the updater will try to treat that AppImage
/// as a deb and fail with "update is not a valid deb package".
#[tauri::command]
pub fn get_updater_target() -> Option<String> {
    // Only override on Linux; other platforms can rely on defaults.
    #[cfg(not(target_os = "linux"))]
    {
        return None;
    }

    #[cfg(target_os = "linux")]
    {
        // AppImage runs set APPIMAGE; prefer explicit appimage target.
        if std::env::var_os("APPIMAGE").is_some() {
            let target = match std::env::consts::ARCH {
                "x86_64" => "linux-x86_64-appimage",
                "aarch64" => "linux-aarch64-appimage",
                _ => return None,
            };
            return Some(target.to_string());
        }

        // Detect deb-installed binary path.
        if let Ok(exe) = std::env::current_exe() {
            if exe == std::path::Path::new("/usr/bin/tandem") {
                let target = match std::env::consts::ARCH {
                    "x86_64" => "linux-x86_64-deb",
                    "aarch64" => "linux-aarch64-deb",
                    _ => return None,
                };
                return Some(target.to_string());
            }
        }

        None
    }
}
