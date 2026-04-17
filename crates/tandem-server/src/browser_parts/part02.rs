#[async_trait]
impl Tool for BrowserTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(self.kind)
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        match self.execute_impl(args).await {
            Ok(result) => Ok(result),
            Err(err) => {
                let message = err.to_string();
                let (code, detail) = split_error_code(&message);
                Ok(error_tool_result(code, detail.to_string(), None))
            }
        }
    }
}

impl RuntimeState {
    pub async fn browser_status(&self) -> BrowserStatus {
        self.browser.status_snapshot().await
    }

    pub async fn browser_smoke_test(
        &self,
        url: Option<String>,
    ) -> anyhow::Result<BrowserSmokeTestResult> {
        self.browser.smoke_test(url).await
    }

    pub async fn install_browser_sidecar(&self) -> anyhow::Result<BrowserSidecarInstallResult> {
        self.browser.install_sidecar().await
    }

    pub async fn browser_health_summary(&self) -> BrowserHealthSummary {
        self.browser.health_summary().await
    }

    pub async fn close_browser_sessions_for_owner(&self, owner_session_id: &str) -> usize {
        self.browser
            .close_sessions_for_owner(owner_session_id)
            .await
    }

    pub async fn close_all_browser_sessions(&self) -> usize {
        self.browser.close_all_sessions().await
    }
}

impl AppState {
    pub async fn browser_status(&self) -> BrowserStatus {
        match self.runtime.get() {
            Some(runtime) => runtime.browser.status_snapshot().await,
            None => BrowserStatus::default(),
        }
    }

    pub async fn browser_smoke_test(
        &self,
        url: Option<String>,
    ) -> anyhow::Result<BrowserSmokeTestResult> {
        let Some(runtime) = self.runtime.get() else {
            anyhow::bail!("runtime not ready");
        };
        runtime.browser_smoke_test(url).await
    }

    pub async fn install_browser_sidecar(&self) -> anyhow::Result<BrowserSidecarInstallResult> {
        let Some(runtime) = self.runtime.get() else {
            anyhow::bail!("runtime not ready");
        };
        runtime.install_browser_sidecar().await
    }

    pub async fn browser_health_summary(&self) -> BrowserHealthSummary {
        match self.runtime.get() {
            Some(runtime) => runtime.browser.health_summary().await,
            None => BrowserHealthSummary::default(),
        }
    }

    pub async fn close_browser_sessions_for_owner(&self, owner_session_id: &str) -> usize {
        match self.runtime.get() {
            Some(runtime) => {
                runtime
                    .close_browser_sessions_for_owner(owner_session_id)
                    .await
            }
            None => 0,
        }
    }

    pub async fn close_all_browser_sessions(&self) -> usize {
        match self.runtime.get() {
            Some(runtime) => runtime.close_all_browser_sessions().await,
            None => 0,
        }
    }

    pub async fn register_browser_tools(&self) -> anyhow::Result<()> {
        let Some(runtime) = self.runtime.get() else {
            anyhow::bail!("runtime not ready");
        };
        runtime
            .browser
            .register_tools(&runtime.tools, Some(self.clone()))
            .await
    }
}

fn evaluate_browser_status(config: BrowserConfig) -> BrowserStatus {
    let mut status = run_doctor(BrowserDoctorOptions {
        enabled: config.enabled,
        headless_default: config.headless_default,
        allow_no_sandbox: config.allow_no_sandbox,
        executable_path: config.executable_path.clone(),
        user_data_root: config.user_data_root.clone(),
    });
    status.headless_default = config.headless_default;
    status.sidecar = evaluate_sidecar_status(config.sidecar_path.as_deref());
    if config.enabled && !status.sidecar.found {
        status.blocking_issues.push(BrowserBlockingIssue {
            code: "browser_sidecar_not_found".to_string(),
            message: "The tandem-browser sidecar binary was not found on this host.".to_string(),
        });
        status.recommendations.push(
            "Install or bundle `tandem-browser`, or set `TANDEM_BROWSER_SIDECAR` / `browser.sidecar_path`."
                .to_string(),
        );
    }
    status.runnable = config.enabled
        && status.sidecar.found
        && status.browser.found
        && status.blocking_issues.is_empty();
    status
}

fn evaluate_sidecar_status(explicit: Option<&str>) -> tandem_browser::BrowserSidecarStatus {
    let path = detect_sidecar_binary_path(explicit);
    let version = path
        .as_ref()
        .and_then(|candidate| probe_binary_version(candidate).ok());
    tandem_browser::BrowserSidecarStatus {
        found: path.is_some(),
        path: path.map(|row| row.to_string_lossy().to_string()),
        version,
    }
}

fn probe_binary_version(path: &Path) -> anyhow::Result<String> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to query `{}` version", path.display()))?;
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

pub async fn install_browser_sidecar(
    config: &BrowserConfig,
) -> anyhow::Result<BrowserSidecarInstallResult> {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let release = fetch_release_for_version(&version).await?;
    let asset_name = browser_release_asset_name()?;
    let asset = release
        .assets
        .iter()
        .find(|candidate| candidate.name == asset_name)
        .ok_or_else(|| {
            anyhow!(
                "release_missing_asset: `{}` not found in {}",
                asset_name,
                release.tag_name
            )
        })?;
    let install_path = sidecar_install_path(config)?;
    let parent = install_path
        .parent()
        .ok_or_else(|| anyhow!("invalid install path `{}`", install_path.display()))?;
    fs::create_dir_all(parent)
        .await
        .with_context(|| format!("failed to create `{}`", parent.display()))?;

    let archive_bytes = download_release_asset(asset).await?;
    let downloaded_bytes = archive_bytes.len() as u64;
    let install_path_for_unpack = install_path.clone();
    let asset_name_for_unpack = asset.name.clone();
    let unpacked = tokio::task::spawn_blocking(move || {
        unpack_sidecar_archive(
            &asset_name_for_unpack,
            &archive_bytes,
            &install_path_for_unpack,
        )
    })
    .await
    .context("browser sidecar install task failed")??;

    let status = evaluate_browser_status(config.clone());
    Ok(BrowserSidecarInstallResult {
        version,
        asset_name: asset.name.clone(),
        installed_path: unpacked.to_string_lossy().to_string(),
        downloaded_bytes: asset.size.max(downloaded_bytes),
        status,
    })
}

async fn fetch_release_for_version(version: &str) -> anyhow::Result<GitHubRelease> {
    let base = std::env::var(RELEASES_URL_ENV)
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{RELEASE_REPO}/releases/tags"));
    let url = format!("{}/v{}", base.trim_end_matches('/'), version);
    let response = reqwest::Client::new()
        .get(&url)
        .header(reqwest::header::USER_AGENT, BROWSER_INSTALL_USER_AGENT)
        .send()
        .await
        .with_context(|| format!("failed to fetch release metadata from `{url}`"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!("release_lookup_failed: {} {}", status, body.trim());
    }
    serde_json::from_str::<GitHubRelease>(&body).context("invalid release metadata payload")
}

async fn download_release_asset(asset: &GitHubAsset) -> anyhow::Result<Vec<u8>> {
    let response = reqwest::Client::new()
        .get(&asset.browser_download_url)
        .header(reqwest::header::USER_AGENT, BROWSER_INSTALL_USER_AGENT)
        .send()
        .await
        .with_context(|| format!("failed to download `{}`", asset.browser_download_url))?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "asset_download_failed: {} {}",
            status,
            asset.browser_download_url
        );
    }
    let bytes = response
        .bytes()
        .await
        .context("failed to read asset bytes")?;
    Ok(bytes.to_vec())
}

fn sidecar_install_path(config: &BrowserConfig) -> anyhow::Result<PathBuf> {
    if let Some(explicit) = config
        .sidecar_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(PathBuf::from(explicit));
    }
    managed_sidecar_install_path()
}

fn managed_sidecar_install_path() -> anyhow::Result<PathBuf> {
    let root = resolve_shared_paths()
        .map(|paths| paths.canonical_root)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|home| home.join(".tandem"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        });
    Ok(root.join("binaries").join(sidecar_binary_name()))
}

fn browser_release_asset_name() -> anyhow::Result<String> {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        anyhow::bail!("unsupported_os: {}", std::env::consts::OS);
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        anyhow::bail!("unsupported_arch: {}", std::env::consts::ARCH);
    };
    let ext = if cfg!(target_os = "windows") || cfg!(target_os = "macos") {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(format!("tandem-browser-{os}-{arch}.{ext}"))
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

fn unpack_sidecar_archive(
    asset_name: &str,
    archive_bytes: &[u8],
    install_path: &Path,
) -> anyhow::Result<PathBuf> {
    if asset_name.ends_with(".zip") {
        let cursor = std::io::Cursor::new(archive_bytes);
        let mut archive = zip::ZipArchive::new(cursor).context("invalid zip archive")?;
        let binary_present = archive
            .file_names()
            .any(|name| name == sidecar_binary_name());
        let mut file = if binary_present {
            archive
                .by_name(sidecar_binary_name())
                .context("browser binary missing from zip archive")?
        } else {
            archive
                .by_index(0)
                .context("browser binary missing from zip archive")?
        };
        let mut output = std::fs::File::create(install_path)
            .with_context(|| format!("failed to create `{}`", install_path.display()))?;
        std::io::copy(&mut file, &mut output).context("failed to unpack zip asset")?;
    } else if asset_name.ends_with(".tar.gz") {
        let cursor = std::io::Cursor::new(archive_bytes);
        let decoder = GzDecoder::new(cursor);
        let mut archive = tar::Archive::new(decoder);
        let mut found = false;
        for entry in archive.entries().context("invalid tar archive")? {
            let mut entry = entry.context("invalid tar entry")?;
            let path = entry.path().context("invalid tar entry path")?;
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == sidecar_binary_name())
            {
                entry
                    .unpack(install_path)
                    .with_context(|| format!("failed to unpack `{}`", install_path.display()))?;
                found = true;
                break;
            }
        }
        if !found {
            anyhow::bail!("browser binary missing from tar archive");
        }
    } else {
        anyhow::bail!("unsupported archive format `{asset_name}`");
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = std::fs::metadata(install_path)
            .with_context(|| format!("failed to read `{}` metadata", install_path.display()))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(install_path, perms)
            .with_context(|| format!("failed to chmod `{}`", install_path.display()))?;
    }

    Ok(install_path.to_path_buf())
}

fn parse_tool_context(args: &Value) -> BrowserToolContext {
    serde_json::from_value(args.clone()).unwrap_or(BrowserToolContext {
        model_session_id: None,
    })
}

fn ok_tool_result(value: Value, metadata: Value) -> anyhow::Result<ToolResult> {
    Ok(ToolResult {
        output: serde_json::to_string_pretty(&value)?,
        metadata,
    })
}

fn error_tool_result(code: &str, message: String, metadata: Option<Value>) -> ToolResult {
    let mut meta = metadata.unwrap_or_else(|| json!({}));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("ok".to_string(), Value::Bool(false));
        obj.insert("code".to_string(), Value::String(code.to_string()));
        obj.insert("message".to_string(), Value::String(message.clone()));
    }
    ToolResult {
        output: message,
        metadata: meta,
    }
}

fn split_error_code(message: &str) -> (&str, &str) {
    let Some((code, detail)) = message.split_once(':') else {
        return ("browser_error", message);
    };
    let code = code.trim();
    if code.is_empty()
        || !code
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch == '_' || ch.is_ascii_digit())
    {
        return ("browser_error", message);
    }
    (code, detail.trim())
}

fn smoke_excerpt(content: &str, max_chars: usize) -> String {
    let mut excerpt = String::new();
    for ch in content.chars().take(max_chars) {
        excerpt.push(ch);
    }
    if content.chars().count() > max_chars {
        excerpt.push_str("...");
    }
    excerpt
}

fn browser_not_runnable_result(status: &BrowserStatus) -> anyhow::Result<ToolResult> {
    ok_tool_result(
        serde_json::to_value(status)?,
        json!({
            "ok": false,
            "code": "browser_not_runnable",
            "runnable": status.runnable,
            "enabled": status.enabled,
        }),
    )
}

fn normalize_allowed_hosts(hosts: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for host in hosts {
        let normalized = host.trim().trim_start_matches('.').to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if !out.iter().any(|existing| existing == &normalized) {
            out.push(normalized);
        }
    }
    out
}

fn browser_url_host(url: &str) -> anyhow::Result<String> {
    let parsed =
        reqwest::Url::parse(url).with_context(|| format!("invalid browser url `{}`", url))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("url `{}` has no host", url))?;
    Ok(host.to_ascii_lowercase())
}

fn ensure_allowed_browser_url(url: &str, allow_hosts: &[String]) -> anyhow::Result<()> {
    let parsed =
        reqwest::Url::parse(url).with_context(|| format!("invalid browser url `{}`", url))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => anyhow::bail!("unsupported_url_scheme: `{}` is not allowed", other),
    }
    if allow_hosts.is_empty() {
        return Ok(());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("url `{}` has no host", url))?
        .to_ascii_lowercase();
    let allowed = allow_hosts
        .iter()
        .any(|candidate| host == *candidate || host.ends_with(&format!(".{candidate}")));
    if !allowed {
        anyhow::bail!("host `{}` is not in the browser allowlist", host);
    }
    Ok(())
}

fn bool_env_value(enabled: bool) -> &'static str {
    if enabled {
        "true"
    } else {
        "false"
    }
}

fn normalize_browser_open_request(request: &mut BrowserOpenRequest) {
    request.profile_id = request
        .profile_id
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
}

fn parse_browser_wait_condition(
    input: BrowserWaitConditionArgs,
) -> anyhow::Result<BrowserWaitCondition> {
    let BrowserWaitConditionArgs {
        kind,
        value,
        selector,
        text,
        url,
    } = input;

    let kind = kind
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| selector.as_ref().map(|_| "selector".to_string()))
        .or_else(|| text.as_ref().map(|_| "text".to_string()))
        .or_else(|| url.as_ref().map(|_| "url".to_string()))
        .ok_or_else(|| anyhow!("browser_wait requires condition.kind"))?;

    let value = value
        .filter(|value| !value.trim().is_empty())
        .or_else(|| match kind.as_str() {
            "selector" => selector,
            "text" => text,
            "url" => url,
            _ => None,
        });

    Ok(BrowserWaitCondition { kind, value })
}

fn parse_browser_wait_args(args: &Value) -> anyhow::Result<BrowserWaitParams> {
    let raw: BrowserWaitToolArgs = serde_json::from_value(args.clone())?;
    let condition = if let Some(condition) = raw.condition {
        parse_browser_wait_condition(condition)?
    } else {
        parse_browser_wait_condition(BrowserWaitConditionArgs {
            kind: raw.kind,
            value: raw.value,
            selector: raw.selector,
            text: raw.text,
            url: raw.url,
        })?
    };

    Ok(BrowserWaitParams {
        session_id: raw.session_id,
        condition,
        timeout_ms: raw.timeout_ms,
    })
}

fn is_local_or_private_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return false;
    };
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.octets()[0] == 169 && ip.octets()[1] == 254
        }
        IpAddr::V6(ip) => {
            ip == Ipv6Addr::LOCALHOST || ip.is_unique_local() || ip.is_unicast_link_local()
        }
    }
}

fn resolve_text_input(text: Option<String>, secret_ref: Option<String>) -> anyhow::Result<String> {
    if let Some(secret_ref) = secret_ref
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        let value = std::env::var(&secret_ref).with_context(|| {
            format!("secret_ref `{}` is not set in the environment", secret_ref)
        })?;
        if value.trim().is_empty() {
            anyhow::bail!("secret_ref `{}` resolved to an empty value", secret_ref);
        }
        return Ok(value);
    }
    let text = text.unwrap_or_default();
    if text.is_empty() {
        anyhow::bail!("browser_type requires either `text` or `secret_ref`");
    }
    Ok(text)
}

fn extension_for_extract_format(format: &str) -> &'static str {
    match format {
        "html" => "html",
        "markdown" => "md",
        _ => "txt",
    }
}

fn viewport_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "width": { "type": "integer", "minimum": 1, "maximum": 10000 },
            "height": { "type": "integer", "minimum": 1, "maximum": 10000 }
        }
    })
}

fn wait_condition_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["selector", "text", "url", "network_idle", "navigation"]
            },
            "value": { "type": "string" }
        },
        "required": ["kind"]
    })
}

fn tool_schema(kind: BrowserToolKind) -> ToolSchema {
    match kind {
        BrowserToolKind::Status => ToolSchema::new(
            "browser_status",
            "Check browser automation readiness and install guidance. Call this first when browser tools may be unavailable.",
            json!({ "type": "object", "properties": {} }),
        ),
        BrowserToolKind::Open => ToolSchema::new(
            "browser_open",
            "Open a URL in a browser session. Only http/https are allowed. Omit profile_id for an ephemeral session.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "profile_id": { "type": "string" },
                    "headless": { "type": "boolean" },
                    "viewport": viewport_schema(),
                    "wait_until": { "type": "string", "enum": ["navigation", "network_idle"] }
                },
                "required": ["url"]
            }),
        ),
        BrowserToolKind::Navigate => ToolSchema::new(
            "browser_navigate",
            "Navigate an existing browser session to a new URL.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "url": { "type": "string" },
                    "wait_until": { "type": "string", "enum": ["navigation", "network_idle"] }
                },
                "required": ["session_id", "url"]
            }),
        ),
        BrowserToolKind::Snapshot => ToolSchema::new(
            "browser_snapshot",
            "Capture a bounded page summary with stable element_id values. Call this before click/type on a new page or after navigation.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "max_elements": { "type": "integer", "minimum": 1, "maximum": 200 },
                    "include_screenshot": { "type": "boolean" }
                },
                "required": ["session_id"]
            }),
        ),
        BrowserToolKind::Click => ToolSchema::new(
            "browser_click",
            "Click a visible page element by element_id when possible. Use wait_for to make navigation and selector waits race-free.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "element_id": { "type": "string" },
                    "selector": { "type": "string" },
                    "wait_for": wait_condition_schema(),
                    "timeout_ms": { "type": "integer", "minimum": 250, "maximum": 120000 }
                },
                "required": ["session_id"]
            }),
        ),
        BrowserToolKind::Type => ToolSchema::new(
            "browser_type",
            "Type text into an element. Prefer secret_ref over text for credentials; secret_ref resolves from the host environment and is redacted from logs.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "element_id": { "type": "string" },
                    "selector": { "type": "string" },
                    "text": { "type": "string" },
                    "secret_ref": { "type": "string" },
                    "replace": { "type": "boolean" },
                    "submit": { "type": "boolean" },
                    "timeout_ms": { "type": "integer", "minimum": 250, "maximum": 120000 }
                },
                "required": ["session_id"]
            }),
        ),
        BrowserToolKind::Press => ToolSchema::new(
            "browser_press",
            "Dispatch a key press in the active page context.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "key": { "type": "string" },
                    "wait_for": wait_condition_schema(),
                    "timeout_ms": { "type": "integer", "minimum": 250, "maximum": 120000 }
                },
                "required": ["session_id", "key"]
            }),
        ),
        BrowserToolKind::Wait => ToolSchema::new(
            "browser_wait",
            "Wait for a selector, text, URL fragment, navigation, or network idle.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "condition": wait_condition_schema(),
                    "wait_for": wait_condition_schema(),
                    "waitFor": wait_condition_schema(),
                    "kind": {
                        "type": "string",
                        "enum": ["selector", "text", "url", "network_idle", "navigation"]
                    },
                    "type": {
                        "type": "string",
                        "enum": ["selector", "text", "url", "network_idle", "navigation"]
                    },
                    "value": { "type": "string" },
                    "selector": { "type": "string" },
                    "text": { "type": "string" },
                    "url": { "type": "string" },
                    "timeout_ms": { "type": "integer", "minimum": 250, "maximum": 120000 },
                    "timeoutMs": { "type": "integer", "minimum": 250, "maximum": 120000 }
                },
                "required": ["session_id"],
                "anyOf": [
                    { "required": ["condition"] },
                    { "required": ["wait_for"] },
                    { "required": ["waitFor"] },
                    { "required": ["kind"] },
                    { "required": ["type"] },
                    { "required": ["selector"] },
                    { "required": ["text"] },
                    { "required": ["url"] }
                ]
            }),
        ),
        BrowserToolKind::Extract => ToolSchema::new(
            "browser_extract",
            "Extract page content as visible_text, markdown, or html. Prefer this over screenshots when you need text.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "format": { "type": "string", "enum": ["visible_text", "markdown", "html"] },
                    "max_bytes": { "type": "integer", "minimum": 1024, "maximum": 2000000 }
                },
                "required": ["session_id", "format"]
            }),
        ),
        BrowserToolKind::Screenshot => ToolSchema::new(
            "browser_screenshot",
            "Capture a screenshot and store it as a browser artifact.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "full_page": { "type": "boolean" },
                    "label": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        ),
        BrowserToolKind::Close => ToolSchema::new(
            "browser_close",
            "Close a browser session and release its resources.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        ),
    }
}
