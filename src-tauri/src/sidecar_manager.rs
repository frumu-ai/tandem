// Sidecar binary management - version tracking, downloads, updates
use crate::error::{Result, TandemError};
use chrono::Utc;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

const ENGINE_REPO: &str = "frumu-ai/tandem-engine";
const GITHUB_API: &str = "https://api.github.com";
const MIN_BINARY_SIZE: u64 = 100 * 1024; // 100KB minimum
const SKIPPED_RELEASE_TAGS: &[&str] = &[];
const RELEASES_PER_PAGE: usize = 20;
const MAX_RELEASE_PAGES: usize = 5;
const RELEASE_CHECK_INTERVAL_SECS: i64 = 6 * 60 * 60;
const RELEASE_CACHE_FILE: &str = "sidecar_release_cache.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarStatus {
    pub installed: bool,
    pub version: Option<String>,
    #[serde(rename = "latestVersion")]
    pub latest_version: Option<String>,
    #[serde(rename = "latestOverallVersion")]
    pub latest_overall_version: Option<String>,
    #[serde(rename = "updateAvailable")]
    pub update_available: bool,
    #[serde(rename = "compatibilityMessage")]
    pub compatibility_message: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReleaseCache {
    fetched_at_unix: i64,
    etag: Option<String>,
    last_modified: Option<String>,
    releases: Vec<GitHubRelease>,
}

struct CompatibleRelease<'a> {
    release: &'a GitHubRelease,
    skipped_count: usize,
    first_skip_reason: Option<String>,
}

/// Get the sidecar binary path for the current platform
/// Checks for updated version in AppData first, falls back to bundled version
pub fn get_sidecar_binary_path(app: &AppHandle) -> Result<PathBuf> {
    let binary_name = get_binary_name();

    // 1. Check AppData (user downloads/updates)
    if let Ok(app_data_dir) = app.path().app_data_dir() {
        let updated_binary = app_data_dir.join("binaries").join(binary_name);
        if updated_binary.exists() {
            tracing::debug!("Using updated sidecar from AppData: {:?}", updated_binary);
            return Ok(updated_binary);
        }
    }

    // 2. Check bundled resources (installed app)
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled_binary = resource_dir.join("binaries").join(binary_name);
        if bundled_binary.exists() {
            tracing::debug!("Using bundled sidecar from resources: {:?}", bundled_binary);
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
            let dev_binary_nested = current_dir
                .join("src-tauri")
                .join("binaries")
                .join(binary_name);
            if dev_binary_nested.exists() {
                tracing::info!(
                    "Using dev sidecar from src-tauri/binaries: {:?}",
                    dev_binary_nested
                );
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
    return "tandem-engine.exe";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "tandem-engine";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "tandem-engine";

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "tandem-engine";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "tandem-engine";
}

/// Get the asset name for the current platform
fn get_asset_name() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return "tandem-engine-windows-x64.zip";

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    return "tandem-engine-darwin-x64.zip";

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    return "tandem-engine-darwin-arm64.zip";

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return "tandem-engine-linux-x64.tar.gz";

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    return "tandem-engine-linux-arm64.tar.gz";
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

    let release_discovery = fetch_release_discovery(app).await.ok();
    let latest_version = release_discovery
        .as_ref()
        .and_then(|discovery| discovery.latest_compatible_release.as_ref())
        .map(|release| release.tag_name.clone());
    let latest_overall_version = release_discovery
        .as_ref()
        .and_then(|discovery| discovery.latest_overall_release.as_ref())
        .map(|release| release.tag_name.clone());
    let compatibility_message = release_discovery
        .as_ref()
        .and_then(build_compatibility_message);

    let update_available = should_offer_update(version.as_deref(), latest_version.as_deref());

    Ok(SidecarStatus {
        installed,
        version,
        latest_version,
        latest_overall_version,
        update_available,
        compatibility_message,
        binary_path: binary_path.map(|p| p.to_string_lossy().to_string()),
    })
}

struct ReleaseDiscovery {
    latest_overall_release: Option<GitHubRelease>,
    latest_compatible_release: Option<GitHubRelease>,
    latest_overall_ineligible_reason: Option<String>,
}

async fn fetch_release_discovery(app: &AppHandle) -> Result<ReleaseDiscovery> {
    let client = reqwest::Client::new();
    let releases = fetch_releases(app, &client, false).await?;
    Ok(build_release_discovery(&releases, beta_channel_enabled()))
}

fn build_release_discovery(
    releases: &[GitHubRelease],
    include_prerelease: bool,
) -> ReleaseDiscovery {
    let latest_overall_release = releases.iter().find(|release| !release.draft).cloned();
    let latest_overall_ineligible_reason = latest_overall_release
        .as_ref()
        .and_then(|release| release_ineligible_reason(release, include_prerelease));

    let latest_compatible_release = releases
        .iter()
        .find(|release| release_ineligible_reason(release, include_prerelease).is_none())
        .cloned();

    ReleaseDiscovery {
        latest_overall_release,
        latest_compatible_release,
        latest_overall_ineligible_reason,
    }
}

fn build_compatibility_message(discovery: &ReleaseDiscovery) -> Option<String> {
    let latest_overall = discovery.latest_overall_release.as_ref()?;
    let latest_compatible = discovery.latest_compatible_release.as_ref();

    if let Some(reason) = &discovery.latest_overall_ineligible_reason {
        return Some(match latest_compatible {
            Some(compatible) => format!(
                "Latest release {} is incompatible ({reason}). Latest compatible release: {}.",
                latest_overall.tag_name, compatible.tag_name
            ),
            None => format!(
                "Latest release {} is incompatible ({reason}), and no compatible release was found.",
                latest_overall.tag_name
            ),
        });
    }

    None
}

async fn fetch_releases(
    app: &AppHandle,
    client: &reqwest::Client,
    force_refresh: bool,
) -> Result<Vec<GitHubRelease>> {
    let now = Utc::now().timestamp();
    let cached = load_release_cache(app);

    if let Some(cache) = &cached {
        let age = now - cache.fetched_at_unix;
        if !force_refresh && age < RELEASE_CHECK_INTERVAL_SECS {
            tracing::debug!(
                "Using cached tandem-engine releases (age={}s, freshness={}s)",
                age,
                RELEASE_CHECK_INTERVAL_SECS
            );
            return Ok(cache.releases.clone());
        }
    }

    let mut request = build_release_request(client, 1);
    if let Some(cache) = &cached {
        if let Some(etag) = &cache.etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(last_modified) = &cache.last_modified {
            request = request.header("If-Modified-Since", last_modified);
        }
    }

    let response = request.send().await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            if let Some(cache) = &cached {
                tracing::warn!(
                    "Failed to refresh releases ({}). Falling back to cached releases.",
                    error
                );
                return Ok(cache.releases.clone());
            }
            return Err(TandemError::Sidecar(format!(
                "Failed to fetch releases: {}",
                error
            )));
        }
    };

    if response.status().as_u16() == 304 {
        if let Some(mut cache) = cached {
            cache.fetched_at_unix = now;
            save_release_cache(app, &cache);
            return Ok(cache.releases);
        }
    }

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());
        tracing::error!("GitHub API error {}: {}", status, error_text);

        if let Some(cache) = &cached {
            tracing::warn!("Using cached releases after GitHub API error {}", status);
            return Ok(cache.releases.clone());
        }

        let message = if status.as_u16() == 403 {
            "GitHub rate limit reached. Please wait a few minutes and try again.".to_string()
        } else {
            format!(
                "Unable to fetch releases (error {}). Please try again later.",
                status.as_u16()
            )
        };
        return Err(TandemError::Sidecar(message));
    }

    let etag = response
        .headers()
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let last_modified = response
        .headers()
        .get("last-modified")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let mut releases = parse_release_response(response).await?;

    if releases.len() >= RELEASES_PER_PAGE {
        for page in 2..=MAX_RELEASE_PAGES {
            let page_response = build_release_request(client, page)
                .send()
                .await
                .map_err(|e| {
                    TandemError::Sidecar(format!("Failed to fetch releases page {}: {}", page, e))
                })?;

            if !page_response.status().is_success() {
                tracing::warn!(
                    "Stopping release pagination at page {} due to status {}",
                    page,
                    page_response.status()
                );
                break;
            }

            let page_releases = parse_release_response(page_response).await?;
            if page_releases.is_empty() {
                break;
            }

            let page_count = page_releases.len();
            releases.extend(page_releases);
            if page_count < RELEASES_PER_PAGE {
                break;
            }
        }
    }

    let cache = ReleaseCache {
        fetched_at_unix: now,
        etag,
        last_modified,
        releases: releases.clone(),
    };
    save_release_cache(app, &cache);

    Ok(releases)
}

fn build_release_request(client: &reqwest::Client, page: usize) -> reqwest::RequestBuilder {
    let url = format!(
        "{}/repos/{}/releases?per_page={}&page={}",
        GITHUB_API, ENGINE_REPO, RELEASES_PER_PAGE, page
    );
    let mut request = client
        .get(url)
        .header("User-Agent", "Tandem-App")
        .header("Accept", "application/vnd.github+json");

    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        request = request.header("Authorization", format!("Bearer {}", token));
    }

    request
}

async fn parse_release_response(response: reqwest::Response) -> Result<Vec<GitHubRelease>> {
    let text = response
        .text()
        .await
        .map_err(|e| TandemError::Sidecar(format!("Failed to read response: {}", e)))?;

    serde_json::from_str(&text).map_err(|e| {
        tracing::error!("Failed to parse GitHub releases response: {}", e);
        tracing::debug!(
            "Response text (first 500 chars): {}",
            &text[..text.len().min(500)]
        );
        TandemError::Sidecar(format!("Failed to parse releases: {}", e))
    })
}

fn select_latest_compatible_release<'a>(
    releases: &'a [GitHubRelease],
    include_prerelease: bool,
) -> Result<CompatibleRelease<'a>> {
    let mut skipped_count = 0usize;
    let mut first_skip_reason: Option<String> = None;

    for release in releases {
        if let Some(reason) = release_ineligible_reason(release, include_prerelease) {
            tracing::debug!(
                tag = %release.tag_name,
                reason = %reason,
                "Skipping tandem-engine release during eligibility filtering"
            );
            skipped_count += 1;
            if first_skip_reason.is_none() {
                first_skip_reason = Some(format!("{} ({})", release.tag_name, reason));
            }
            continue;
        }

        return Ok(CompatibleRelease {
            release,
            skipped_count,
            first_skip_reason,
        });
    }

    let reason = first_skip_reason.unwrap_or_else(|| "no releases returned".to_string());
    Err(TandemError::Sidecar(format!(
        "No compatible tandem-engine release found. First skip reason: {}",
        reason
    )))
}

fn release_ineligible_reason(release: &GitHubRelease, include_prerelease: bool) -> Option<String> {
    if release.draft {
        return Some("draft release".to_string());
    }

    if release.prerelease && !include_prerelease {
        return Some("prerelease".to_string());
    }

    if is_tag_skipped(&release.tag_name) {
        return Some("manually skipped tag".to_string());
    }

    let missing_assets = missing_required_assets(release);
    if !missing_assets.is_empty() {
        return Some(format!(
            "missing required headless asset(s): {}",
            missing_assets.join(", ")
        ));
    }

    None
}

fn missing_required_assets(release: &GitHubRelease) -> Vec<&'static str> {
    let required_asset = get_asset_name();
    if release
        .assets
        .iter()
        .any(|asset| asset_name_matches_current_target(&asset.name))
    {
        Vec::new()
    } else {
        vec![required_asset]
    }
}

fn is_tag_skipped(tag: &str) -> bool {
    tag_in_skip_list(SKIPPED_RELEASE_TAGS, tag)
}

fn tag_in_skip_list(skipped_tags: &[&str], tag: &str) -> bool {
    let normalized = normalize_version_label(tag);
    skipped_tags
        .iter()
        .any(|skipped| normalize_version_label(skipped) == normalized)
}

fn log_release_selection(context: &str, selected: &CompatibleRelease<'_>) {
    tracing::info!(
        context = context,
        selected_tag = %selected.release.tag_name,
        skipped_count = selected.skipped_count,
        first_skip_reason = selected.first_skip_reason.as_deref().unwrap_or("none"),
        "Selected eligible tandem-engine release"
    );
}

fn should_offer_update(installed_version: Option<&str>, latest_version: Option<&str>) -> bool {
    match (installed_version, latest_version) {
        (Some(current), Some(latest)) => is_version_newer(latest, current),
        (None, Some(_)) => true,
        _ => false,
    }
}

fn is_version_newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == Ordering::Greater
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    match (parse_semver(left), parse_semver(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => compare_numeric_fallback(left, right),
    }
}

fn parse_semver(version: &str) -> Option<Version> {
    let normalized = normalize_version_label(version).trim();
    if normalized.is_empty() {
        return None;
    }
    Version::parse(normalized).ok()
}

fn compare_numeric_fallback(left: &str, right: &str) -> Ordering {
    let parse = |version: &str| {
        normalize_version_label(version)
            .split('.')
            .map(|segment| {
                segment
                    .chars()
                    .take_while(|ch| ch.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>()
    };

    let left_parts = parse(left);
    let right_parts = parse(right);
    let max_len = left_parts.len().max(right_parts.len());
    (0..max_len)
        .map(|index| {
            left_parts
                .get(index)
                .copied()
                .unwrap_or(0)
                .cmp(&right_parts.get(index).copied().unwrap_or(0))
        })
        .find(|ordering| *ordering != Ordering::Equal)
        .unwrap_or(Ordering::Equal)
}

fn normalize_version_label(version: &str) -> &str {
    version.trim_start_matches(|c| c == 'v' || c == 'V')
}

fn asset_name_matches_current_target(asset_name: &str) -> bool {
    if !asset_name.starts_with("tandem-engine-") {
        return false;
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return asset_name.contains("windows") && asset_name.contains("x64");
    }

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return asset_name.contains("darwin") && asset_name.contains("x64");
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return asset_name.contains("darwin") && asset_name.contains("arm64");
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return asset_name.contains("linux") && asset_name.contains("x64");
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return asset_name.contains("linux") && asset_name.contains("arm64");
    }
}

fn beta_channel_enabled() -> bool {
    std::env::var("TANDEM_OPENCODE_UPDATE_CHANNEL")
        .map(|value| value.eq_ignore_ascii_case("beta"))
        .unwrap_or(false)
}

fn get_release_cache_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(RELEASE_CACHE_FILE))
}

fn load_release_cache(app: &AppHandle) -> Option<ReleaseCache> {
    let path = get_release_cache_path(app)?;
    let data = fs::read_to_string(path).ok()?;
    serde_json::from_str::<ReleaseCache>(&data).ok()
}

fn save_release_cache(app: &AppHandle, cache: &ReleaseCache) {
    let Some(path) = get_release_cache_path(app) else {
        return;
    };

    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            tracing::warn!("Failed to create release cache dir: {}", error);
            return;
        }
    }

    match serde_json::to_string(cache) {
        Ok(data) => {
            if let Err(error) = fs::write(path, data) {
                tracing::warn!("Failed to save release cache: {}", error);
            }
        }
        Err(error) => tracing::warn!("Failed to serialize release cache: {}", error),
    }
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

    let client = reqwest::Client::new();
    let releases = fetch_releases(&app, &client, true).await.map_err(|e| {
        let error_msg = e.to_string();
        emit_state("error", Some(&error_msg));
        e
    })?;
    let selected =
        select_latest_compatible_release(&releases, beta_channel_enabled()).map_err(|e| {
            let error_msg = e.to_string();
            emit_state("error", Some(&error_msg));
            e
        })?;
    log_release_selection("sidecar_download", &selected);

    let release = selected.release;
    let asset_name = get_asset_name();
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .or_else(|| {
            release
                .assets
                .iter()
                .find(|asset| asset_name_matches_current_target(&asset.name))
        })
        .ok_or_else(|| {
            let err = format!(
                "Selected release {} is missing compatible headless asset for this platform",
                release.tag_name
            );
            emit_state("error", Some(&err));
            TandemError::Sidecar(err)
        })?;

    let version = release.tag_name.clone();
    let download_url = asset.browser_download_url.clone();
    let total_size = asset.size;

    tracing::info!(
        "Downloading tandem-engine {} from {}",
        version,
        download_url
    );

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
        "tandem-engine.exe"
    } else {
        "tandem-engine"
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

                tracing::info!(
                    "Running taskkill to ensure all tandem-engine processes are terminated"
                );

                // Kill any tandem-engine.exe processes by name
                let mut cmd = StdCommand::new("taskkill");
                cmd.args(["/F", "/IM", "tandem-engine.exe"]);

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
                cmd2.args(["/F", "/FI", "IMAGENAME eq tandem-engine*"]);

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

    tracing::info!("tandem-engine {} installed successfully", version);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_release(tag: &str, draft: bool, prerelease: bool, assets: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.to_string(),
            draft,
            prerelease,
            assets: assets
                .iter()
                .map(|name| GitHubAsset {
                    name: (*name).to_string(),
                    browser_download_url: format!("https://example.com/{}", name),
                    size: 123,
                })
                .collect(),
        }
    }

    fn current_platform_assets() -> Vec<&'static str> {
        vec![get_asset_name()]
    }

    #[test]
    fn select_latest_compatible_release_prefers_newest_valid_release() {
        let releases = vec![
            make_release("v1.1.58", false, false, &current_platform_assets()),
            make_release("v1.1.57", false, false, &current_platform_assets()),
        ];
        let selected = select_latest_compatible_release(&releases, false).unwrap();
        assert_eq!(selected.release.tag_name, "v1.1.58");
    }

    #[test]
    fn select_latest_compatible_release_skips_missing_required_assets() {
        let releases = vec![
            make_release("v1.1.58", false, false, &["tandem-desktop-windows-x64.exe"]),
            make_release("v1.1.57", false, false, &current_platform_assets()),
        ];
        let selected = select_latest_compatible_release(&releases, false).unwrap();
        assert_eq!(selected.release.tag_name, "v1.1.57");
    }

    #[test]
    fn manual_skip_list_normalizes_versions() {
        assert!(tag_in_skip_list(&["v1.1.59"], "1.1.59"));
        assert!(tag_in_skip_list(&["1.1.59"], "v1.1.59"));
        assert!(!tag_in_skip_list(&["v1.1.58"], "v1.1.59"));
    }

    #[test]
    fn select_latest_compatible_release_rejects_desktop_only_release() {
        let desktop_only = vec![
            "tandem-desktop-windows-x64.exe",
            "tandem-desktop-darwin-x64.dmg",
            "tandem-desktop-linux-amd64.deb",
        ];
        let releases = vec![
            make_release("v1.1.58", false, false, &desktop_only),
            make_release("v1.1.57", false, false, &current_platform_assets()),
        ];
        let selected = select_latest_compatible_release(&releases, false).unwrap();
        assert_eq!(selected.release.tag_name, "v1.1.57");
    }

    #[test]
    fn select_latest_compatible_release_rejects_draft_and_prerelease() {
        let releases = vec![
            make_release("v1.1.60", true, false, &current_platform_assets()),
            make_release("v1.1.59", false, true, &current_platform_assets()),
            make_release("v1.1.58", false, false, &current_platform_assets()),
        ];
        let selected = select_latest_compatible_release(&releases, false).unwrap();
        assert_eq!(selected.release.tag_name, "v1.1.58");
    }

    #[test]
    fn select_latest_compatible_release_errors_when_none_eligible() {
        let releases = vec![
            make_release("v1.1.59", false, false, &["tandem-desktop-windows-x64.exe"]),
            make_release("v1.1.58", true, false, &current_platform_assets()),
        ];
        assert!(select_latest_compatible_release(&releases, false).is_err());
    }

    #[test]
    fn release_discovery_reports_incompatible_latest() {
        let releases = vec![
            make_release("v1.1.59", false, false, &["tandem-desktop-windows-x64.exe"]),
            make_release("v1.1.58", false, false, &current_platform_assets()),
        ];
        let discovery = build_release_discovery(&releases, false);
        assert_eq!(
            discovery
                .latest_overall_release
                .as_ref()
                .map(|r| r.tag_name.as_str()),
            Some("v1.1.59")
        );
        assert_eq!(
            discovery
                .latest_compatible_release
                .as_ref()
                .map(|r| r.tag_name.as_str()),
            Some("v1.1.58")
        );
        assert!(build_compatibility_message(&discovery).is_some());
    }

    #[test]
    fn version_comparison_identifies_newer_release() {
        assert!(is_version_newer("1.1.58", "1.1.57"));
    }

    #[test]
    fn version_comparison_treats_v_prefix_as_equal() {
        assert!(!is_version_newer("v1.1.58", "1.1.58"));
        assert_eq!(compare_versions("v1.1.58", "1.1.58"), Ordering::Equal);
    }

    #[test]
    fn semver_comparison_handles_pre_release() {
        assert!(is_version_newer("1.2.0", "1.2.0-beta.1"));
    }

    #[test]
    fn version_comparison_prevents_downgrade_prompt() {
        assert!(!is_version_newer("1.1.58", "1.1.59"));
        assert!(!should_offer_update(Some("1.1.59"), Some("1.1.58")));
    }
}
