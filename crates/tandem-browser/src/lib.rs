use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context};
use base64::Engine;
use headless_chrome::browser::tab::Tab;
use headless_chrome::{Browser, LaunchOptionsBuilder};
use html2md::parse_html;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tempfile::TempDir;
use uuid::Uuid;

pub const BROWSER_PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserViewport {
    pub width: u32,
    pub height: u32,
}

impl Default for BrowserViewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserBlockingIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserSidecarStatus {
    pub found: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserExecutableStatus {
    pub found: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserStatus {
    pub enabled: bool,
    pub runnable: bool,
    #[serde(default)]
    pub headless_default: bool,
    #[serde(default)]
    pub sidecar: BrowserSidecarStatus,
    #[serde(default)]
    pub browser: BrowserExecutableStatus,
    #[serde(default)]
    pub blocking_issues: Vec<BrowserBlockingIssue>,
    #[serde(default)]
    pub recommendations: Vec<String>,
    #[serde(default)]
    pub install_hints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Default for BrowserStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            runnable: false,
            headless_default: true,
            sidecar: BrowserSidecarStatus::default(),
            browser: BrowserExecutableStatus::default(),
            blocking_issues: Vec::new(),
            recommendations: Vec::new(),
            install_hints: Vec::new(),
            last_checked_at_ms: Some(now_ms()),
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserArtifactRef {
    pub artifact_id: String,
    pub uri: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserElementRef {
    pub element_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_hint: Option<String>,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub editable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserSnapshotResult {
    pub session_id: String,
    pub url: String,
    pub title: String,
    pub load_state: String,
    pub viewport: BrowserViewport,
    #[serde(default)]
    pub elements: Vec<BrowserElementRef>,
    #[serde(default)]
    pub notices: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserWaitCondition {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserWaitParams {
    pub session_id: String,
    pub condition: BrowserWaitCondition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserOpenRequest {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headless: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<BrowserViewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_data_root: Option<String>,
    #[serde(default)]
    pub allow_no_sandbox: bool,
    #[serde(default)]
    pub headless_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserOpenResult {
    pub session_id: String,
    pub final_url: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_version: Option<String>,
    pub headless: bool,
    pub viewport: BrowserViewport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserNavigateParams {
    pub session_id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserNavigateResult {
    pub session_id: String,
    pub final_url: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserSnapshotParams {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_elements: Option<usize>,
    #[serde(default)]
    pub include_screenshot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserClickParams {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_for: Option<BrowserWaitCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserTypeParams {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    pub text: String,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub submit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserPressParams {
    pub session_id: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_for: Option<BrowserWaitCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserActionResult {
    pub session_id: String,
    pub success: bool,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserExtractParams {
    pub session_id: String,
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserExtractResult {
    pub session_id: String,
    pub format: String,
    pub content: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserScreenshotParams {
    pub session_id: String,
    #[serde(default)]
    pub full_page: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserScreenshotResult {
    pub session_id: String,
    pub mime_type: String,
    pub data_base64: String,
    pub bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserCloseParams {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserCloseResult {
    pub session_id: String,
    pub closed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserDoctorOptions {
    pub enabled: bool,
    #[serde(default)]
    pub headless_default: bool,
    #[serde(default)]
    pub allow_no_sandbox: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_data_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BrowserRpcError>,
}

impl BrowserRpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i64, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(BrowserRpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BrowserServerOptions {
    pub executable_path: Option<String>,
    pub user_data_root: Option<String>,
    pub allow_no_sandbox: bool,
    pub headless_default: bool,
}

impl Default for BrowserServerOptions {
    fn default() -> Self {
        Self {
            executable_path: env::var("TANDEM_BROWSER_EXECUTABLE").ok(),
            user_data_root: env::var("TANDEM_BROWSER_USER_DATA_ROOT").ok(),
            allow_no_sandbox: env::var("TANDEM_BROWSER_ALLOW_NO_SANDBOX")
                .ok()
                .and_then(|raw| parse_bool_like(&raw))
                .unwrap_or(false),
            headless_default: env::var("TANDEM_BROWSER_HEADLESS")
                .ok()
                .and_then(|raw| parse_bool_like(&raw))
                .unwrap_or(true),
        }
    }
}

struct BrowserSession {
    _browser: Browser,
    tab: Arc<Tab>,
    viewport: BrowserViewport,
    _headless: bool,
    _browser_version: Option<String>,
    _profile_dir: Option<TempDir>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn current_sidecar_status() -> BrowserSidecarStatus {
    let path = env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    BrowserSidecarStatus {
        found: path.is_some(),
        path,
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    }
}

pub fn parse_bool_like(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn detect_browser_executable(explicit: Option<&str>) -> Option<PathBuf> {
    let mut candidates = Vec::<PathBuf>::new();
    if let Some(path) = explicit.map(str::trim).filter(|v| !v.is_empty()) {
        candidates.push(PathBuf::from(path));
    }

    let names = if cfg!(target_os = "windows") {
        vec!["chrome.exe", "msedge.exe", "brave.exe", "chromium.exe"]
    } else if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "google-chrome",
            "microsoft-edge",
            "brave-browser",
            "chromium",
        ]
    } else {
        vec![
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
            "brave-browser",
            "brave",
        ]
    };

    for name in names {
        let candidate = PathBuf::from(name);
        if candidate.is_absolute() {
            candidates.push(candidate);
        } else if let Some(found) = find_on_path(name) {
            candidates.push(found);
        }
    }

    if cfg!(target_os = "windows") {
        for raw in [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
            r"C:\Program Files (x86)\BraveSoftware\Brave-Browser\Application\brave.exe",
        ] {
            candidates.push(PathBuf::from(raw));
        }
    }

    candidates
        .into_iter()
        .find(|path| path.exists() && path.is_file())
}

pub fn detect_sidecar_binary_path(explicit: Option<&str>) -> Option<PathBuf> {
    let mut candidates = Vec::<PathBuf>::new();
    if let Some(raw) = explicit.map(str::trim).filter(|v| !v.is_empty()) {
        candidates.push(PathBuf::from(raw));
    }
    if let Ok(raw) = env::var("TANDEM_BROWSER_SIDECAR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(sidecar_binary_name()));
            candidates.push(parent.join("..").join(sidecar_binary_name()));
            candidates.push(parent.join("..").join("..").join(sidecar_binary_name()));
        }
    }
    if let Some(path) = find_on_path(sidecar_binary_name()) {
        candidates.push(path);
    }
    candidates
        .into_iter()
        .find(|path| path.exists() && path.is_file())
}

fn sidecar_binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "tandem-browser.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "tandem-browser"
    }
}

pub fn run_doctor(options: BrowserDoctorOptions) -> BrowserStatus {
    let mut status = BrowserStatus {
        enabled: options.enabled,
        runnable: false,
        headless_default: options.headless_default,
        sidecar: current_sidecar_status(),
        browser: BrowserExecutableStatus::default(),
        blocking_issues: Vec::new(),
        recommendations: Vec::new(),
        install_hints: Vec::new(),
        last_checked_at_ms: Some(now_ms()),
        last_error: None,
    };

    if !options.enabled {
        status.blocking_issues.push(BrowserBlockingIssue {
            code: "disabled_by_config".to_string(),
            message: "Browser automation is disabled by configuration.".to_string(),
        });
        status
            .recommendations
            .push("Set `browser.enabled=true` to enable browser automation.".to_string());
        return status;
    }

    let browser_path = detect_browser_executable(options.executable_path.as_deref());
    let Some(browser_path) = browser_path else {
        status.blocking_issues.push(BrowserBlockingIssue {
            code: "browser_not_found".to_string(),
            message: "No compatible Chromium-based browser executable was found.".to_string(),
        });
        status.recommendations.push(
            "Install Chrome, Chromium, Edge, or Brave on the same machine as tandem-engine."
                .to_string(),
        );
        status.install_hints = linux_install_hints();
        status.recommendations.push(
            "Set `TANDEM_BROWSER_EXECUTABLE` or `browser.executable_path` if the browser is installed in a non-standard location."
                .to_string(),
        );
        return status;
    };

    status.browser.found = true;
    status.browser.path = Some(browser_path.to_string_lossy().to_string());
    status.browser.channel = Some(detect_browser_channel(&browser_path));

    match browser_version(&browser_path) {
        Ok(version) => status.browser.version = Some(version),
        Err(err) => {
            status.blocking_issues.push(BrowserBlockingIssue {
                code: "browser_not_executable".to_string(),
                message: format!(
                    "Found browser executable at `{}`, but failed to query version: {}",
                    browser_path.display(),
                    truncate(&err.to_string(), 200)
                ),
            });
            status.last_error = Some(err.to_string());
            return status;
        }
    }

    match smoke_test_browser(
        &browser_path,
        options.allow_no_sandbox,
        options.user_data_root.as_deref().map(Path::new),
        options.headless_default,
    ) {
        Ok(version) => {
            if status.browser.version.is_none() {
                status.browser.version = version;
            }
            status.runnable = true;
        }
        Err(err) => {
            let (code, message) = classify_launch_error(&err);
            status
                .blocking_issues
                .push(BrowserBlockingIssue { code, message });
            status.last_error = Some(err.to_string());
            status.recommendations.push(
                "Run `tandem-browser doctor --json` on the host to inspect full browser readiness diagnostics."
                    .to_string(),
            );
            if matches!(
                status.blocking_issues.last().map(|row| row.code.as_str()),
                Some("missing_shared_libraries")
            ) {
                status.install_hints = linux_install_hints();
            }
        }
    }

    status
}

fn browser_version(path: &Path) -> anyhow::Result<String> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to launch `{}` for version probe", path.display()))?;
    if !output.status.success() {
        anyhow::bail!(
            "version probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        anyhow::bail!("version probe returned empty stdout");
    }
    Ok(stdout)
}

fn smoke_test_browser(
    browser_path: &Path,
    allow_no_sandbox: bool,
    user_data_root: Option<&Path>,
    headless_default: bool,
) -> anyhow::Result<Option<String>> {
    let mut launch = LaunchOptionsBuilder::default();
    let profile_dir = if let Some(root) = user_data_root {
        fs::create_dir_all(root)
            .with_context(|| format!("failed to create `{}`", root.display()))?;
        let root = root.join(format!("doctor-{}", Uuid::new_v4()));
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create `{}`", root.display()))?;
        Some(root)
    } else {
        None
    };

    launch
        .path(Some(browser_path.to_path_buf()))
        .headless(headless_default)
        .sandbox(!allow_no_sandbox)
        .window_size(Some((1280, 800)));
    if let Some(path) = profile_dir.as_ref() {
        launch.user_data_dir(Some(path.to_path_buf()));
    }
    let browser = Browser::new(
        launch
            .build()
            .map_err(|err| anyhow!("failed to build launch options: {}", err))?,
    )?;
    let tab = browser.new_tab()?;
    tab.navigate_to("about:blank")?;
    tab.wait_until_navigated()?;
    let _ = tab.evaluate("document.readyState", false)?;
    let version = browser
        .get_version()
        .ok()
        .map(|v| v.product)
        .filter(|v| !v.trim().is_empty());
    drop(tab);
    drop(browser);
    if let Some(path) = profile_dir {
        let _ = fs::remove_dir_all(path);
    }
    Ok(version)
}

fn classify_launch_error(err: &anyhow::Error) -> (String, String) {
    let raw = err.to_string().to_ascii_lowercase();
    if raw.contains("sandbox") {
        return (
            "sandbox_unavailable".to_string(),
            "Chromium launch failed because sandbox support is unavailable on this host."
                .to_string(),
        );
    }
    if raw.contains("shared libraries")
        || raw.contains("libnss3")
        || raw.contains("libatk")
        || raw.contains("error while loading shared libraries")
    {
        return (
            "missing_shared_libraries".to_string(),
            "Chromium is installed, but required shared libraries are missing.".to_string(),
        );
    }
    if raw.contains("permission denied") {
        return (
            "browser_not_executable".to_string(),
            "Configured browser path exists, but is not executable.".to_string(),
        );
    }
    (
        "browser_launch_failed".to_string(),
        format!(
            "Failed to launch Chromium: {}",
            truncate(&err.to_string(), 220)
        ),
    )
}

fn linux_install_hints() -> Vec<String> {
    if !cfg!(target_os = "linux") {
        return Vec::new();
    }
    let os_release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let distro = Regex::new(r#"(?m)^ID="?([^"\n]+)"?"#)
        .ok()
        .and_then(|re| re.captures(&os_release))
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    match distro.as_str() {
        "ubuntu" | "debian" => vec![
            "Install Chromium or Chrome with apt, then set TANDEM_BROWSER_EXECUTABLE if needed.".to_string(),
            "Example: sudo apt update && sudo apt install -y chromium".to_string(),
        ],
        "fedora" | "rhel" | "centos" => vec![
            "Install Chromium with dnf, then set TANDEM_BROWSER_EXECUTABLE if needed.".to_string(),
            "Example: sudo dnf install -y chromium".to_string(),
        ],
        "arch" | "manjaro" => vec![
            "Install Chromium with pacman, then set TANDEM_BROWSER_EXECUTABLE if needed.".to_string(),
            "Example: sudo pacman -S chromium".to_string(),
        ],
        "alpine" => vec![
            "Install Chromium and required fonts/libs with apk.".to_string(),
            "Example: sudo apk add chromium nss freetype harfbuzz ca-certificates ttf-freefont".to_string(),
        ],
        _ => vec![
            "Install a Chromium-based browser on this host and set TANDEM_BROWSER_EXECUTABLE if it is not on PATH.".to_string(),
        ],
    }
}

fn detect_browser_channel(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("edge") {
        "edge".to_string()
    } else if name.contains("brave") {
        "brave".to_string()
    } else if name.contains("chromium") {
        "chromium".to_string()
    } else {
        "chrome".to_string()
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.exists() && candidate.is_file())
}

fn truncate(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let mut out = text.chars().take(max_len).collect::<String>();
    out.push_str("...");
    out
}

fn sanitize_profile_id(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("profile_id cannot be empty");
    }
    let cleaned = trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if cleaned.is_empty() {
        anyhow::bail!("profile_id is invalid");
    }
    Ok(cleaned)
}

fn ensure_http_url(url: &str) -> anyhow::Result<()> {
    let trimmed = url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(());
    }
    anyhow::bail!("unsupported URL scheme; only http and https are allowed")
}

fn resolve_user_data_root(explicit: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(raw) = explicit.map(str::trim).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(raw);
        fs::create_dir_all(&path)?;
        return Ok(path);
    }
    if let Some(base) = dirs::data_local_dir() {
        let path = base.join("tandem").join("browser");
        fs::create_dir_all(&path)?;
        return Ok(path);
    }
    let path = env::current_dir()?.join(".tandem-browser");
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn wait_for_condition(
    tab: &Arc<Tab>,
    url_reader: impl Fn() -> anyhow::Result<String>,
    condition: Option<BrowserWaitCondition>,
    timeout_ms: Option<u64>,
) -> anyhow::Result<()> {
    let Some(condition) = condition else {
        return Ok(());
    };
    let deadline =
        Instant::now() + Duration::from_millis(timeout_ms.unwrap_or(15_000).clamp(250, 120_000));
    loop {
        match condition.kind.as_str() {
            "selector" => {
                let selector = condition
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow!("wait_for.selector requires condition.value"))?;
                if element_exists(tab, selector)? {
                    return Ok(());
                }
            }
            "text" => {
                let needle = condition
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow!("wait_for.text requires condition.value"))?;
                let body_text =
                    evaluate_string(tab, "document.body ? document.body.innerText || '' : ''")?;
                if body_text.contains(needle) {
                    return Ok(());
                }
            }
            "url" => {
                let needle = condition
                    .value
                    .as_deref()
                    .ok_or_else(|| anyhow!("wait_for.url requires condition.value"))?;
                if url_reader()?.contains(needle) {
                    return Ok(());
                }
            }
            "navigation" => {
                tab.wait_until_navigated()?;
                return Ok(());
            }
            "network_idle" => {
                let state = evaluate_string(tab, "document.readyState")?;
                if state == "complete" {
                    thread::sleep(Duration::from_millis(500));
                    return Ok(());
                }
            }
            other => anyhow::bail!("unsupported wait condition kind `{}`", other),
        }

        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for `{}` condition", condition.kind);
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn element_exists(tab: &Arc<Tab>, selector: &str) -> anyhow::Result<bool> {
    let script = format!(
        "Boolean(document.querySelector({}))",
        serde_json::to_string(selector)?
    );
    Ok(evaluate_bool(tab, &script)?)
}

fn evaluate_bool(tab: &Arc<Tab>, script: &str) -> anyhow::Result<bool> {
    tab.evaluate(script, false)?
        .value
        .and_then(|v| v.as_bool())
        .ok_or_else(|| anyhow!("script did not return a boolean"))
}

fn evaluate_string(tab: &Arc<Tab>, script: &str) -> anyhow::Result<String> {
    tab.evaluate(script, false)?
        .value
        .and_then(|v| v.as_str().map(ToString::to_string))
        .ok_or_else(|| anyhow!("script did not return a string"))
}

fn evaluate_json(tab: &Arc<Tab>, script: &str) -> anyhow::Result<Value> {
    tab.evaluate(script, false)?
        .value
        .ok_or_else(|| anyhow!("script did not return a JSON value"))
}

fn tab_url(tab: &Arc<Tab>) -> anyhow::Result<String> {
    evaluate_string(tab, "window.location.href")
}

fn tab_title(tab: &Arc<Tab>) -> anyhow::Result<String> {
    evaluate_string(tab, "document.title")
}

fn selector_from_ref(element_id: Option<&str>, selector: Option<&str>) -> anyhow::Result<String> {
    if let Some(raw) = element_id.map(str::trim).filter(|v| !v.is_empty()) {
        return Ok(format!(r#"[data-tandem-browser-id="{}"]"#, raw));
    }
    if let Some(raw) = selector.map(str::trim).filter(|v| !v.is_empty()) {
        return Ok(raw.to_string());
    }
    anyhow::bail!("either element_id or selector is required")
}

fn clear_element_value(tab: &Arc<Tab>, selector: &str) -> anyhow::Result<()> {
    let script = format!(
        r#"(function() {{
            const el = document.querySelector({selector});
            if (!el) {{
                return false;
            }}
            if ("value" in el) {{
                el.value = "";
            }}
            el.textContent = "";
            return true;
        }})()"#,
        selector = serde_json::to_string(selector)?,
    );
    if !evaluate_bool(tab, &script)? {
        anyhow::bail!("selector `{}` not found", selector);
    }
    Ok(())
}

fn submit_element(tab: &Arc<Tab>, selector: &str) -> anyhow::Result<()> {
    let script = format!(
        r#"(function() {{
            const el = document.querySelector({selector});
            if (!el) {{
                return false;
            }}
            if (el.form && typeof el.form.requestSubmit === "function") {{
                el.form.requestSubmit();
                return true;
            }}
            if (el.form) {{
                el.form.submit();
                return true;
            }}
            el.dispatchEvent(new KeyboardEvent("keydown", {{ key: "Enter", bubbles: true }}));
            el.dispatchEvent(new KeyboardEvent("keyup", {{ key: "Enter", bubbles: true }}));
            return true;
        }})()"#,
        selector = serde_json::to_string(selector)?,
    );
    if !evaluate_bool(tab, &script)? {
        anyhow::bail!("selector `{}` not found", selector);
    }
    Ok(())
}

fn dispatch_key(tab: &Arc<Tab>, key: &str) -> anyhow::Result<()> {
    let script = format!(
        r#"(function() {{
            const key = {key};
            const target = document.activeElement || document.body;
            if (!target) {{
                return false;
            }}
            target.dispatchEvent(new KeyboardEvent("keydown", {{ key, bubbles: true }}));
            target.dispatchEvent(new KeyboardEvent("keyup", {{ key, bubbles: true }}));
            return true;
        }})()"#,
        key = serde_json::to_string(key)?,
    );
    if !evaluate_bool(tab, &script)? {
        anyhow::bail!("failed to dispatch key `{}`", key);
    }
    Ok(())
}

fn snapshot_script(max_elements: usize) -> anyhow::Result<String> {
    Ok(format!(
        r#"(function() {{
            const maxElements = {max_elements};
            const selectorHint = (el) => {{
                if (el.id) return `#${{el.id}}`;
                if (el.getAttribute("name")) return `${{el.tagName.toLowerCase()}}[name="${{el.getAttribute("name")}}"]`;
                if (el.getAttribute("type")) return `${{el.tagName.toLowerCase()}}[type="${{el.getAttribute("type")}}"]`;
                if (el.getAttribute("role")) return `${{el.tagName.toLowerCase()}}[role="${{el.getAttribute("role")}}"]`;
                return el.tagName.toLowerCase();
            }};
            const visible = (el) => {{
                const rect = el.getBoundingClientRect();
                const style = window.getComputedStyle(el);
                return !!style && style.visibility !== "hidden" && style.display !== "none" && rect.width > 0 && rect.height > 0;
            }};
            const role = (el) => el.getAttribute("role") || (["A"].includes(el.tagName) ? "link" : null) || (["BUTTON"].includes(el.tagName) ? "button" : null);
            const textOf = (el) => (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 240);
            const nameOf = (el) => el.getAttribute("aria-label") || el.getAttribute("name") || el.getAttribute("placeholder") || textOf(el) || null;
            const elements = [];
            const seen = new Set();
            const nodes = Array.from(document.querySelectorAll("a,button,input,textarea,select,[role],[tabindex],[contenteditable='true']"));
            for (const el of nodes) {{
                if (!visible(el)) continue;
                if (seen.has(el)) continue;
                seen.add(el);
                if (!el.dataset.tandemBrowserId) {{
                    el.dataset.tandemBrowserId = `tb-${{Math.random().toString(36).slice(2, 10)}}`;
                }}
                elements.push({{
                    element_id: el.dataset.tandemBrowserId,
                    role: role(el),
                    name: nameOf(el),
                    text: textOf(el) || null,
                    selector_hint: selectorHint(el),
                    visible: true,
                    enabled: !el.disabled,
                    editable: !!(el.isContentEditable || ["INPUT", "TEXTAREA"].includes(el.tagName)),
                    checked: typeof el.checked === "boolean" ? !!el.checked : null,
                    bounds: {{
                        x: Math.round(el.getBoundingClientRect().x),
                        y: Math.round(el.getBoundingClientRect().y),
                        width: Math.round(el.getBoundingClientRect().width),
                        height: Math.round(el.getBoundingClientRect().height)
                    }}
                }});
                if (elements.length >= maxElements) break;
            }}
            return {{
                url: window.location.href,
                title: document.title || "",
                load_state: document.readyState || "unknown",
                elements,
                notices: []
            }};
        }})()"#,
    ))
}

pub fn run_stdio_server(options: BrowserServerOptions) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut sessions = HashMap::<String, BrowserSession>::new();

    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<BrowserRpcRequest>(trimmed) {
            Ok(request) => request,
            Err(err) => {
                let response = BrowserRpcResponse::err(
                    Value::Null,
                    -32700,
                    format!("invalid JSON-RPC request: {}", err),
                    None,
                );
                writeln!(writer, "{}", serde_json::to_string(&response)?)?;
                writer.flush()?;
                continue;
            }
        };
        let response = handle_request(&options, &mut sessions, request);
        writeln!(writer, "{}", serde_json::to_string(&response)?)?;
        writer.flush()?;
    }

    sessions.clear();
    Ok(())
}

fn handle_request(
    options: &BrowserServerOptions,
    sessions: &mut HashMap<String, BrowserSession>,
    request: BrowserRpcRequest,
) -> BrowserRpcResponse {
    let id = request.id.clone();
    let run = || -> anyhow::Result<Value> {
        match request.method.as_str() {
            "browser.version" => Ok(json!({
                "protocol_version": BROWSER_PROTOCOL_VERSION,
                "sidecar_version": env!("CARGO_PKG_VERSION"),
            })),
            "browser.doctor" => {
                let params: BrowserDoctorOptions = serde_json::from_value(request.params)?;
                Ok(serde_json::to_value(run_doctor(params))?)
            }
            "browser.open" => {
                let params: BrowserOpenRequest = serde_json::from_value(request.params)?;
                let result = open_session(options, sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.navigate" => {
                let params: BrowserNavigateParams = serde_json::from_value(request.params)?;
                let result = navigate_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.snapshot" => {
                let params: BrowserSnapshotParams = serde_json::from_value(request.params)?;
                let result = snapshot_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.click" => {
                let params: BrowserClickParams = serde_json::from_value(request.params)?;
                let result = click_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.type" => {
                let params: BrowserTypeParams = serde_json::from_value(request.params)?;
                let result = type_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.press" => {
                let params: BrowserPressParams = serde_json::from_value(request.params)?;
                let result = press_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.wait" => {
                let params: BrowserWaitParams = serde_json::from_value(request.params)?;
                let result = wait_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.extract" => {
                let params: BrowserExtractParams = serde_json::from_value(request.params)?;
                let result = extract_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.screenshot" => {
                let params: BrowserScreenshotParams = serde_json::from_value(request.params)?;
                let result = screenshot_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.close" => {
                let params: BrowserCloseParams = serde_json::from_value(request.params)?;
                let result = close_session(sessions, params)?;
                Ok(serde_json::to_value(result)?)
            }
            "browser.ping" => Ok(json!({ "ok": true })),
            other => anyhow::bail!("unknown method `{}`", other),
        }
    };

    match run() {
        Ok(result) => BrowserRpcResponse::ok(id, result),
        Err(err) => {
            let message = err.to_string();
            let code = if message.contains("session") && message.contains("not found") {
                404
            } else if message.contains("selector") && message.contains("not found") {
                422
            } else {
                500
            };
            BrowserRpcResponse::err(
                id,
                code,
                message,
                Some(json!({ "protocol_version": BROWSER_PROTOCOL_VERSION })),
            )
        }
    }
}

fn open_session(
    options: &BrowserServerOptions,
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserOpenRequest,
) -> anyhow::Result<BrowserOpenResult> {
    ensure_http_url(&params.url)?;
    let viewport = params.viewport.unwrap_or_default();
    let headless = params.headless.unwrap_or(options.headless_default);
    if cfg!(target_os = "linux")
        && !headless
        && env::var("DISPLAY").is_err()
        && env::var("WAYLAND_DISPLAY").is_err()
    {
        anyhow::bail!("headed_mode_unavailable: no DISPLAY or WAYLAND_DISPLAY is available");
    }

    let executable = detect_browser_executable(
        params
            .executable_path
            .as_deref()
            .or(options.executable_path.as_deref()),
    )
    .ok_or_else(|| anyhow!("browser_not_found: no Chromium executable found"))?;
    let user_data_root = resolve_user_data_root(
        params
            .user_data_root
            .as_deref()
            .or(options.user_data_root.as_deref()),
    )?;
    let (profile_dir, temp_dir) = if let Some(profile_id) = params.profile_id.as_deref() {
        let profile_id = sanitize_profile_id(profile_id)?;
        let path = user_data_root.join(profile_id);
        fs::create_dir_all(&path)?;
        (path, None)
    } else {
        let dir = tempfile::Builder::new()
            .prefix("tandem-browser-")
            .tempdir_in(user_data_root)?;
        (dir.path().to_path_buf(), Some(dir))
    };

    let mut launch = LaunchOptionsBuilder::default();
    launch
        .path(Some(executable))
        .headless(headless)
        .sandbox(!(params.allow_no_sandbox || options.allow_no_sandbox))
        .window_size(Some((viewport.width, viewport.height)))
        .user_data_dir(Some(profile_dir));
    let browser = Browser::new(
        launch
            .build()
            .map_err(|err| anyhow!("failed to build launch options: {}", err))?,
    )?;
    let browser_version = browser
        .get_version()
        .ok()
        .map(|v| v.product)
        .filter(|v| !v.trim().is_empty());
    let tab = browser.new_tab()?;
    tab.navigate_to(&params.url)?;
    wait_for_condition(
        &tab,
        || tab_url(&tab),
        params
            .wait_until
            .map(|kind| BrowserWaitCondition { kind, value: None }),
        Some(20_000),
    )?;

    let final_url = tab_url(&tab)?;
    let title = tab_title(&tab)?;
    let session_id = format!("browser-{}", Uuid::new_v4());
    sessions.insert(
        session_id.clone(),
        BrowserSession {
            _browser: browser,
            tab,
            viewport: viewport.clone(),
            _headless: headless,
            _browser_version: browser_version.clone(),
            _profile_dir: temp_dir,
        },
    );

    Ok(BrowserOpenResult {
        session_id,
        final_url,
        title,
        browser_version,
        headless,
        viewport,
    })
}

fn with_session<T>(
    sessions: &mut HashMap<String, BrowserSession>,
    session_id: &str,
    f: impl FnOnce(&mut BrowserSession) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| anyhow!("session `{}` not found", session_id))?;
    f(session)
}

fn navigate_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserNavigateParams,
) -> anyhow::Result<BrowserNavigateResult> {
    ensure_http_url(&params.url)?;
    with_session(sessions, &params.session_id, |session| {
        session.tab.navigate_to(&params.url)?;
        wait_for_condition(
            &session.tab,
            || tab_url(&session.tab),
            params.wait_until.as_ref().map(|kind| BrowserWaitCondition {
                kind: kind.clone(),
                value: None,
            }),
            Some(20_000),
        )?;
        Ok(BrowserNavigateResult {
            session_id: params.session_id.clone(),
            final_url: tab_url(&session.tab)?,
            title: tab_title(&session.tab)?,
        })
    })
}

fn snapshot_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserSnapshotParams,
) -> anyhow::Result<BrowserSnapshotResult> {
    with_session(sessions, &params.session_id, |session| {
        let started = Instant::now();
        let raw = evaluate_json(
            &session.tab,
            &snapshot_script(params.max_elements.unwrap_or(50).clamp(1, 200))?,
        )?;
        let mut snapshot: BrowserSnapshotResult = serde_json::from_value(json!({
            "session_id": params.session_id,
            "url": raw.get("url").cloned().unwrap_or_else(|| Value::String(String::new())),
            "title": raw.get("title").cloned().unwrap_or_else(|| Value::String(String::new())),
            "load_state": raw.get("load_state").cloned().unwrap_or_else(|| Value::String("unknown".to_string())),
            "viewport": session.viewport,
            "elements": raw.get("elements").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
            "notices": raw.get("notices").cloned().unwrap_or_else(|| Value::Array(Vec::new()))
        }))?;
        if params.include_screenshot {
            let bytes = session.tab.capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None,
                None,
                true,
            )?;
            snapshot.screenshot_base64 =
                Some(base64::engine::general_purpose::STANDARD.encode(bytes));
        }
        snapshot.notices.push(format!(
            "snapshot_completed_in_ms={}",
            started.elapsed().as_millis()
        ));
        Ok(snapshot)
    })
}

fn click_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserClickParams,
) -> anyhow::Result<BrowserActionResult> {
    with_session(sessions, &params.session_id, |session| {
        let started = Instant::now();
        let selector = selector_from_ref(params.element_id.as_deref(), params.selector.as_deref())?;
        let element = session.tab.wait_for_element(&selector)?;
        element.click()?;
        wait_for_condition(
            &session.tab,
            || tab_url(&session.tab),
            params.wait_for.clone(),
            params.timeout_ms,
        )?;
        Ok(BrowserActionResult {
            session_id: params.session_id.clone(),
            success: true,
            elapsed_ms: started.elapsed().as_millis() as u64,
            final_url: Some(tab_url(&session.tab)?),
            title: Some(tab_title(&session.tab)?),
        })
    })
}

fn type_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserTypeParams,
) -> anyhow::Result<BrowserActionResult> {
    with_session(sessions, &params.session_id, |session| {
        let started = Instant::now();
        let selector = selector_from_ref(params.element_id.as_deref(), params.selector.as_deref())?;
        if params.replace {
            clear_element_value(&session.tab, &selector)?;
        }
        let element = session.tab.wait_for_element(&selector)?;
        element.click()?;
        element.type_into(&params.text)?;
        if params.submit {
            submit_element(&session.tab, &selector)?;
        }
        Ok(BrowserActionResult {
            session_id: params.session_id.clone(),
            success: true,
            elapsed_ms: started.elapsed().as_millis() as u64,
            final_url: Some(tab_url(&session.tab)?),
            title: Some(tab_title(&session.tab)?),
        })
    })
}

fn press_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserPressParams,
) -> anyhow::Result<BrowserActionResult> {
    with_session(sessions, &params.session_id, |session| {
        let started = Instant::now();
        dispatch_key(&session.tab, &params.key)?;
        wait_for_condition(
            &session.tab,
            || tab_url(&session.tab),
            params.wait_for.clone(),
            params.timeout_ms,
        )?;
        Ok(BrowserActionResult {
            session_id: params.session_id.clone(),
            success: true,
            elapsed_ms: started.elapsed().as_millis() as u64,
            final_url: Some(tab_url(&session.tab)?),
            title: Some(tab_title(&session.tab)?),
        })
    })
}

fn wait_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserWaitParams,
) -> anyhow::Result<BrowserActionResult> {
    with_session(sessions, &params.session_id, |session| {
        let started = Instant::now();
        wait_for_condition(
            &session.tab,
            || tab_url(&session.tab),
            Some(params.condition.clone()),
            params.timeout_ms,
        )?;
        Ok(BrowserActionResult {
            session_id: params.session_id.clone(),
            success: true,
            elapsed_ms: started.elapsed().as_millis() as u64,
            final_url: Some(tab_url(&session.tab)?),
            title: Some(tab_title(&session.tab)?),
        })
    })
}

fn extract_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserExtractParams,
) -> anyhow::Result<BrowserExtractResult> {
    with_session(sessions, &params.session_id, |session| {
        let max_bytes = params.max_bytes.unwrap_or(256_000).clamp(1_024, 2_000_000);
        let (format, mut content) = match params.format.as_str() {
            "html" => (
                "html".to_string(),
                evaluate_string(
                    &session.tab,
                    "document.documentElement ? document.documentElement.outerHTML || '' : ''",
                )?,
            ),
            "markdown" => {
                let html = evaluate_string(
                    &session.tab,
                    "document.documentElement ? document.documentElement.outerHTML || '' : ''",
                )?;
                ("markdown".to_string(), parse_html(&html))
            }
            "visible_text" | "text" => (
                "visible_text".to_string(),
                evaluate_string(
                    &session.tab,
                    "document.body ? document.body.innerText || '' : ''",
                )?,
            ),
            other => anyhow::bail!("unsupported extract format `{}`", other),
        };
        let mut truncated = false;
        if content.len() > max_bytes {
            content.truncate(max_bytes);
            truncated = true;
        }
        Ok(BrowserExtractResult {
            session_id: params.session_id.clone(),
            format,
            content,
            truncated,
        })
    })
}

fn screenshot_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserScreenshotParams,
) -> anyhow::Result<BrowserScreenshotResult> {
    with_session(sessions, &params.session_id, |session| {
        let bytes = session.tab.capture_screenshot(
            headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
            None,
            None,
            params.full_page,
        )?;
        Ok(BrowserScreenshotResult {
            session_id: params.session_id.clone(),
            mime_type: "image/png".to_string(),
            bytes: bytes.len(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            label: params.label.clone(),
        })
    })
}

fn close_session(
    sessions: &mut HashMap<String, BrowserSession>,
    params: BrowserCloseParams,
) -> anyhow::Result<BrowserCloseResult> {
    let removed = sessions.remove(&params.session_id);
    Ok(BrowserCloseResult {
        session_id: params.session_id,
        closed: removed.is_some(),
    })
}
