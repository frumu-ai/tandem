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
pub fn get_sidecar_binary_path(app: &AppHandle) -> Result<PathBuf> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| TandemError::Sidecar(format!("Failed to get resource dir: {}", e)))?;

    let binary_name = get_binary_name();
    Ok(resource_dir.join("binaries").join(binary_name))
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
    let binary_path = get_sidecar_binary_path(app)?;
    let installed = binary_path.exists()
        && binary_path
            .metadata()
            .map(|m| m.len() >= MIN_BINARY_SIZE)
            .unwrap_or(false);

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
        binary_path: if installed {
            Some(binary_path.to_string_lossy().to_string())
        } else {
            None
        },
    })
}

/// Fetch the latest release version from GitHub
async fn fetch_latest_version() -> Result<String> {
    let client = reqwest::Client::new();
    let url = format!("{}/repos/{}/releases", GITHUB_API, OPENCODE_REPO);

    let releases: Vec<GitHubRelease> = client
        .get(&url)
        .header("User-Agent", "Tandem-App")
        .send()
        .await
        .map_err(|e| TandemError::Sidecar(format!("Failed to fetch releases: {}", e)))?
        .json()
        .await
        .map_err(|e| TandemError::Sidecar(format!("Failed to parse releases: {}", e)))?;

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

    let releases: Vec<GitHubRelease> = client
        .get(&url)
        .header("User-Agent", "Tandem-App")
        .send()
        .await
        .map_err(|e| {
            emit_state("error", Some(&e.to_string()));
            TandemError::Sidecar(format!("Failed to fetch releases: {}", e))
        })?
        .json()
        .await
        .map_err(|e| {
            emit_state("error", Some(&e.to_string()));
            TandemError::Sidecar(format!("Failed to parse releases: {}", e))
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

    tracing::info!(
        "Downloading OpenCode {} from {}",
        version,
        download_url
    );

    // Create binaries directory
    let binary_path = get_sidecar_binary_path(&app)?;
    if let Some(parent) = binary_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            emit_state("error", Some(&e.to_string()));
            TandemError::Sidecar(format!("Failed to create binaries dir: {}", e))
        })?;
    }

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

    let binaries_dir = binary_path.parent().unwrap();
    extract_archive(&archive_path, binaries_dir, asset_name)?;

    // Rename extracted binary to expected name
    let extracted_name = if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    };
    let extracted_path = binaries_dir.join(extracted_name);

    if extracted_path.exists() && extracted_path != binary_path {
        if binary_path.exists() {
            fs::remove_file(&binary_path).ok();
        }
        fs::rename(&extracted_path, &binary_path).map_err(|e| {
            emit_state("error", Some(&e.to_string()));
            TandemError::Sidecar(format!("Failed to rename binary: {}", e))
        })?;
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

fn extract_archive(archive_path: &PathBuf, dest_dir: &std::path::Path, asset_name: &str) -> Result<()> {
    if asset_name.ends_with(".zip") {
        // Extract zip
        let file = fs::File::open(archive_path)
            .map_err(|e| TandemError::Sidecar(format!("Failed to open archive: {}", e)))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| TandemError::Sidecar(format!("Failed to read zip: {}", e)))?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)
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
        archive.unpack(dest_dir)
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
