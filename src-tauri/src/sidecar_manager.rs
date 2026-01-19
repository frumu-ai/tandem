// Sidecar binary management - version tracking, downloads, updates
use crate::error::{Result, TandemError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

const OPENCODE_REPO: &str = "anomalyco/opencode";
const GITHUB_API: &str = "https://api.github.com";
const MIN_BINARY_SIZE: u64 = 100 * 1024; // 100KB minimum

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarStatus {
    pub installed: bool,
    pub version: Option<String>,
    #[serde(rename = "latestVersion")]
    pub latest_version: Option<String>,
    #[serde(rename = "updateAvailable")]
    pub update_available: bool,
    #[serde(rename = "binaryPath")]
    pub binary_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percent: f32,
    pub speed: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadState {
    pub state: String,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Get the sidecar binary path for the current platform
/// Checks for updated version in AppData first, falls back to bundled version
pub fn get_sidecar_binary_path(app: &AppHandle) -> Result<PathBuf> {
    let binary_name = get_binary_name();

    // 1. Check AppData (user downloads/updates)
    if let Ok(app_data_dir) = app.path().app_data_dir() {
        let updated_binary = app_data_dir.join("binaries").join(binary_name);
        if updated_binary.exists() {
            tracing::info!("Using updated sidecar from AppData: {:?}", updated_binary);
            return Ok(updated_binary);
        }
    }

    // 2. Check bundled resources (installed app)
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled_binary = resource_dir.join("binaries").join(binary_name);
        if bundled_binary.exists() {
            tracing::info!("Using bundled sidecar from resources: {:?}", bundled_binary);
            return Ok(bundled_binary);
        }
    }

    // 3. DEVELOPMENT MODE ONLY: Check source directory
    // In 'tauri dev', resource_dir() might not point where we expect, so we look relative to the crate
    #[cfg(debug_assertions)]
    {
        // Try to find the source root by looking up from the current executable or CWD
        if let Ok(current_dir) = std::env::current_dir() {
             // We assume we are running from 'src-tauri' or project root
             let dev_binary = current_dir.join("binaries").join(binary_name);
             if dev_binary.exists() {
                 tracing::info!("Using dev sidecar from CWD/binaries: {:?}", dev_binary);
                 return Ok(dev_binary);
             }
             
             // Try src-tauri/binaries if we are in the project root
             let dev_binary_nested = current_dir.join("src-tauri").join("binaries").join(binary_name);
             if dev_binary_nested.exists() {
                 tracing::info!("Using dev sidecar from src-tauri/binaries: {:?}", dev_binary_nested);
                 return Ok(dev_binary_nested);
             }
        }
    }

    // Binary not found
    Err(TandemError::Sidecar(format!(
        "Sidecar binary '{}' not found. Please download it first.",
        binary_name
    )))
}

/// Get the binary name for the current platform
fn get_binary_name() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "opencode-x86_64-pc-windows-msvc.exe";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "opencode-x86_64-apple-darwin";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "opencode-aarch64-apple-darwin";

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "opencode-x86_64-unknown-linux-gnu";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "opencode-aarch64-unknown-linux-gnu";
}

/// Get the asset name for the current platform
fn get_asset_name() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "opencode-windows-x64.zip";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "opencode-darwin-x64.zip";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "opencode-darwin-arm64.zip";

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "opencode-linux-x64.tar.gz";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "opencode-linux-arm64.tar.gz";
}

/// Get the installed version from the store
fn get_installed_version(app: &AppHandle) -> Option<String> {
    let store = app.store("settings.json").ok()?;
    store
        .get("sidecar_version")
        .and_then(|v| v.as_str().map(String::from))
}

/// Save the installed version to the store
fn save_installed_version(app: &AppHandle, version: &str) -> Result<()> {
    let store = app
        .store("settings.json")
        .map_err(|e| TandemError::Sidecar(format!("Failed to open store: {}", e)))?;
    store.set("sidecar_version", serde_json::json!(version));
    store
        .save()
        .map_err(|e| TandemError::Sidecar(format!("Failed to save store: {}", e)))?;
    Ok(())
}

/// Check the sidecar status (installed, version, updates)
pub async fn check_sidecar_status(app: &AppHandle) -> Result<SidecarStatus> {
    // Check if binary exists
    let binary_path_result = get_sidecar_binary_path(app);
    let installed = binary_path_result.is_ok()
        && binary_path_result
            .as_ref()
            .unwrap()
            .metadata()
            .map(|m| m.len() >= MIN_BINARY_SIZE)
            .unwrap_or(false);
            
    let binary_path = binary_path_result.ok();

    let version = if installed {
        get_installed_version(app)
    } else {
        None
    };

    // Fetch latest version from GitHub
    let latest_version = fetch_latest_version().await.ok();

    let update_available = match (&version, &latest_version) {
        (Some(current), Some(latest)) => {
            // Simple version comparison (strip 'v' prefix if present)
            let current_clean = current.trim_start_matches('v');
            let latest_clean = latest.trim_start_matches('v');
            current_clean != latest_clean
        }
        (None, Some(_)) => true, // Not installed, update available
        _ => false,
    };

    Ok(SidecarStatus {
        installed,
        version,
        latest_version,
        update_available,
        binary_path: binary_path.map(|p| p.to_string_lossy().to_string()),
    })
}

/// Fetch the latest release version from GitHub
async fn fetch_latest_version() -> Result<String> {
    let client = reqwest::Client::new();
    let url = format!("{}/repos/{}/releases", GITHUB_API, OPENCODE_REPO);

    let mut request = client.get(&url).header("User-Agent", "Tandem-App");

    // Add GitHub token if available (for CI or power users)
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        tracing::debug!("Using GITHUB_TOKEN for authenticated API request");
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    let response = request
        .send()
        .await
        .map_err(|e| TandemError::Sidecar(format!("Failed to fetch releases: {}", e)))?;

    // Check status code
    let status = response.status();
    if !status.is_success() {
        // Handle rate limiting with a user-friendly message
        if status.as_u16() == 403 {
            return Err(TandemError::Sidecar(
                "GitHub rate limit reached. Please wait a few minutes and try again.".to_string(),
            ));
        }

        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());

        tracing::error!("GitHub API error {}: {}", status, error_text);
        return Err(TandemError::Sidecar(format!(
            "Unable to check for updates (error {}). Please try again later.",
            status.as_u16()
        )));
    }

    // Get response text for debugging
    let text = response
        .text()
        .await
        .map_err(|e| TandemError::Sidecar(format!("Failed to read response: {}", e)))?;

    // Try to parse as JSON
    let releases: Vec<GitHubRelease> = serde_json::from_str(&text).map_err(|e| {
        tracing::error!("Failed to parse GitHub releases response: {}", e);
        tracing::debug!(
            "Response text (first 500 chars): {}",
            &text[..text.len().min(500)]
        );
        TandemError::Sidecar(format!("Failed to parse releases: {}", e))
    })?;

    // Find the latest non-draft, non-prerelease with our asset
    let asset_name = get_asset_name();
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        if release.assets.iter().any(|a| a.name == asset_name) {
            return Ok(release.tag_name);
        }
    }

    Err(TandemError::Sidecar(
        "No suitable release found".to_string(),
    ))
}

/// Download the sidecar binary
pub async fn download_sidecar(app: AppHandle) -> Result<()> {
    let emit_state = |state: &str, error: Option<&str>| {
        let _ = app.emit(
            "sidecar-download-state",
            DownloadState {
                state: state.to_string(),
                error: error.map(String::from),
            },
        );
    };

    let emit_progress = |downloaded: u64, total: u64, speed: &str| {
        let percent = if total > 0 {
            (downloaded as f32 / total as f32) * 100.0
        } else {
            0.0
        };
        let _ = app.emit(
            "sidecar-download-progress",
            DownloadProgress {
                downloaded,
                total,
                percent,
                speed: speed.to_string(),
            },
        );
    };

    emit_state("downloading", None);

    // Fetch releases to find the download URL
    let client = reqwest::Client::new();
    let url = format!("{}/repos/{}/releases", GITHUB_API, OPENCODE_REPO);

    let mut request = client.get(&url).header("User-Agent", "Tandem-App");

    // Add GitHub token if available (for CI or power users)
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        tracing::debug!("Using GITHUB_TOKEN for authenticated API request");
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    let response = request.send().await.map_err(|e| {
        emit_state("error", Some(&e.to_string()));
        TandemError::Sidecar(format!("Failed to fetch releases: {}", e))
    })?;

    // Check status code
    let status = response.status();
    if !status.is_success() {
        // Handle rate limiting with a user-friendly message
        if status.as_u16() == 403 {
            let error_msg =
                "GitHub rate limit reached. Please wait a few minutes and try again.".to_string();
            emit_state("error", Some(&error_msg));
            return Err(TandemError::Sidecar(error_msg));
        }

        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());

        tracing::error!("GitHub API error {}: {}", status, error_text);
        let error_msg = format!(
            "Unable to download (error {}). Please try again later.",
            status.as_u16()
        );
        emit_state("error", Some(&error_msg));
        return Err(TandemError::Sidecar(error_msg));
    }

    // Get response text for debugging
    let text = response.text().await.map_err(|e| {
        let error_msg = format!("Failed to read response: {}", e);
        emit_state("error", Some(&error_msg));
        TandemError::Sidecar(error_msg)
    })?;

    // Try to parse as JSON
    let releases: Vec<GitHubRelease> = serde_json::from_str(&text).map_err(|e| {
        tracing::error!("Failed to parse GitHub releases response: {}", e);
        tracing::debug!(
            "Response text (first 500 chars): {}",
            &text[..text.len().min(500)]
        );
        let error_msg = format!("Failed to parse releases: {}", e);
        emit_state("error", Some(&error_msg));
        TandemError::Sidecar(error_msg)
    })?;

    // Find the release with our asset
    let asset_name = get_asset_name();
    let (release, asset) = releases
        .iter()
        .filter(|r| !r.draft && !r.prerelease)
        .find_map(|r| {
            r.assets
                .iter()
                .find(|a| a.name == asset_name)
                .map(|a| (r, a))
        })
        .ok_or_else(|| {
            let err = format!("No release found with asset: {}", asset_name);
            emit_state("error", Some(&err));
            TandemError::Sidecar(err)
        })?;

    let version = release.tag_name.clone();
    let download_url = asset.browser_download_url.clone();
    let total_size = asset.size;

    tracing::info!("Downloading OpenCode {} from {}", version, download_url);

    // Download to AppData (writable), not resources (read-only)
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| TandemError::Sidecar(format!("Failed to get app data dir: {}", e)))?;

    let binaries_dir = app_data_dir.join("binaries");
    fs::create_dir_all(&binaries_dir).map_err(|e| {
        emit_state("error", Some(&e.to_string()));
        TandemError::Sidecar(format!("Failed to create binaries dir: {}", e))
    })?;

    let binary_name = get_binary_name();
    let binary_path = binaries_dir.join(binary_name);

    // Download the archive
    let archive_path = binary_path.with_extension("download");
    let mut response = client
        .get(&download_url)
        .header("User-Agent", "Tandem-App")
        .send()
        .await
        .map_err(|e| {
            emit_state("error", Some(&e.to_string()));
            TandemError::Sidecar(format!("Failed to start download: {}", e))
        })?;

    let mut file = fs::File::create(&archive_path).map_err(|e| {
        emit_state("error", Some(&e.to_string()));
        TandemError::Sidecar(format!("Failed to create file: {}", e))
    })?;

    let mut downloaded: u64 = 0;
    let start_time = std::time::Instant::now();

    while let Some(chunk) = response.chunk().await.map_err(|e| {
        emit_state("error", Some(&e.to_string()));
        TandemError::Sidecar(format!("Download error: {}", e))
    })? {
        file.write_all(&chunk).map_err(|e| {
            emit_state("error", Some(&e.to_string()));
            TandemError::Sidecar(format!("Write error: {}", e))
        })?;

        downloaded += chunk.len() as u64;

        // Calculate speed
        let elapsed = start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 {
            let bytes_per_sec = downloaded as f64 / elapsed;
            format_speed(bytes_per_sec)
        } else {
            String::new()
        };

        emit_progress(downloaded, total_size, &speed);
    }

    drop(file);

    // Extract the archive
    emit_state("extracting", None);
    emit_progress(downloaded, total_size, "Extracting...");

    let binaries_dir = binary_path.parent().unwrap();
    extract_archive(&archive_path, binaries_dir, asset_name)?;

    // Rename extracted binary to expected name
    emit_state("installing", None);

    let extracted_name = if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    };
    let extracted_path = binaries_dir.join(extracted_name);

    if extracted_path.exists() && extracted_path != binary_path {
        // Stop the sidecar before attempting to replace the binary
        if let Some(state) = app.try_state::<crate::state::AppState>() {
            tracing::info!("Stopping sidecar before binary update");
            let _ = state.sidecar.stop().await;

            // On Windows, aggressively kill any remaining processes
            #[cfg(windows)]
            {
                use std::process::Command as StdCommand;

                tracing::info!("Running taskkill to ensure all OpenCode processes are terminated");

                // Kill any opencode.exe processes by name
                let mut cmd = StdCommand::new("taskkill");
                cmd.args(["/F", "/IM", "opencode.exe"]);

                // Hide console window on Windows
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }

                let result = cmd.output();

                match result {
                    Ok(output) => {
                        tracing::info!(
                            "taskkill /IM result: {}",
                            String::from_utf8_lossy(&output.stdout)
                        );
                    }
                    Err(e) => tracing::warn!("Failed to run taskkill /IM: {}", e),
                }

                // Also try killing any process with the executable name in its path
                let mut cmd2 = StdCommand::new("taskkill");
                cmd2.args(["/F", "/FI", "IMAGENAME eq opencode*"]);

                // Hide console window on Windows
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    cmd2.creation_flags(CREATE_NO_WINDOW);
                }

                let result2 = cmd2.output();

                match result2 {
                    Ok(output) => {
                        tracing::info!(
                            "taskkill /FI result: {}",
                            String::from_utf8_lossy(&output.stdout)
                        );
                    }
                    Err(e) => tracing::warn!("Failed to run taskkill /FI: {}", e),
                }
            }

            // Give extra time for Windows to release file locks
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }

        if binary_path.exists() {
            // Try to remove the old binary, retry a few times on Windows
            let mut retries = 5;
            let mut last_error = None;

            while retries > 0 {
                match fs::remove_file(&binary_path) {
                    Ok(_) => {
                        last_error = None;
                        break;
                    }
                    Err(e) => {
                        last_error = Some(e);
                        retries -= 1;
                        if retries > 0 {
                            tracing::debug!("Retry removing old binary, {} attempts left", retries);
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                    }
                }
            }

            if let Some(e) = last_error {
                tracing::warn!(
                    "Failed to remove old binary: {}. Attempting rename anyway.",
                    e
                );
            }
        }

        // Try rename with retry logic
        let mut retries = 5;
        let mut last_error = None;

        while retries > 0 {
            match fs::rename(&extracted_path, &binary_path) {
                Ok(_) => {
                    last_error = None;
                    break;
                }
                Err(e) => {
                    last_error = Some(e);
                    retries -= 1;
                    if retries > 0 {
                        tracing::debug!("Retry renaming binary, {} attempts left", retries);
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
            }
        }

        if let Some(e) = last_error {
            emit_state("error", Some(&e.to_string()));
            return Err(TandemError::Sidecar(format!(
                "Failed to rename binary after 5 attempts. The process may still be running. Please close Tandem completely and try again: {}", 
                e
            )));
        }
    }

    // Set executable permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary_path)
            .map_err(|e| TandemError::Sidecar(format!("Failed to get permissions: {}", e)))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary_path, perms)
            .map_err(|e| TandemError::Sidecar(format!("Failed to set permissions: {}", e)))?;
    }

    // Clean up archive
    fs::remove_file(&archive_path).ok();

    // Save version
    save_installed_version(&app, &version)?;

    emit_state("complete", None);

    tracing::info!("OpenCode {} installed successfully", version);

    Ok(())
}

fn extract_archive(
    archive_path: &PathBuf,
    dest_dir: &std::path::Path,
    asset_name: &str,
) -> Result<()> {
    if asset_name.ends_with(".zip") {
        // Extract zip
        let file = fs::File::open(archive_path)
            .map_err(|e| TandemError::Sidecar(format!("Failed to open archive: {}", e)))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| TandemError::Sidecar(format!("Failed to read zip: {}", e)))?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| TandemError::Sidecar(format!("Failed to read zip entry: {}", e)))?;

            let outpath = dest_dir.join(file.mangled_name());

            if file.is_dir() {
                fs::create_dir_all(&outpath).ok();
            } else {
                if let Some(p) = outpath.parent() {
                    fs::create_dir_all(p).ok();
                }
                let mut outfile = fs::File::create(&outpath)
                    .map_err(|e| TandemError::Sidecar(format!("Failed to create file: {}", e)))?;
                std::io::copy(&mut file, &mut outfile)
                    .map_err(|e| TandemError::Sidecar(format!("Failed to extract file: {}", e)))?;
            }
        }
    } else if asset_name.ends_with(".tar.gz") {
        // Extract tar.gz
        let file = fs::File::open(archive_path)
            .map_err(|e| TandemError::Sidecar(format!("Failed to open archive: {}", e)))?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);
        archive
            .unpack(dest_dir)
            .map_err(|e| TandemError::Sidecar(format!("Failed to extract tar.gz: {}", e)))?;
    }

    Ok(())
}

fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1_000_000.0 {
        format!("{:.1} MB/s", bytes_per_sec / 1_000_000.0)
    } else if bytes_per_sec >= 1_000.0 {
        format!("{:.0} KB/s", bytes_per_sec / 1_000.0)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}
