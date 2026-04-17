use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use base64::Engine;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_browser::{
    detect_sidecar_binary_path, run_doctor, BrowserActionResult, BrowserArtifactRef,
    BrowserBlockingIssue, BrowserCloseParams, BrowserCloseResult, BrowserDoctorOptions,
    BrowserExtractParams, BrowserExtractResult, BrowserNavigateParams, BrowserNavigateResult,
    BrowserOpenRequest, BrowserOpenResult, BrowserPressParams, BrowserRpcRequest,
    BrowserRpcResponse, BrowserScreenshotParams, BrowserScreenshotResult, BrowserSnapshotParams,
    BrowserSnapshotResult, BrowserStatus, BrowserTypeParams, BrowserViewport, BrowserWaitCondition,
    BrowserWaitParams, BROWSER_PROTOCOL_VERSION,
};
use tandem_core::{resolve_shared_paths, BrowserConfig};
use tandem_tools::{Tool, ToolRegistry};
use tandem_types::{EngineEvent, ToolResult, ToolSchema};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{now_ms, AppState, RoutineRunArtifact, RuntimeState};

const STATUS_CACHE_MAX_AGE_MS: u64 = 30_000;
const INLINE_EXTRACT_LIMIT_BYTES: usize = 24_000;
const SNAPSHOT_SCREENSHOT_LABEL: &str = "browser snapshot";
const RELEASE_REPO: &str = "frumu-ai/tandem";
const RELEASES_URL_ENV: &str = "TANDEM_BROWSER_RELEASES_URL";
const BROWSER_INSTALL_USER_AGENT: &str = "tandem-browser-installer";

#[derive(Debug)]
struct BrowserSidecarClient {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    stderr: BufReader<ChildStderr>,
    next_id: u64,
}

#[derive(Debug, Clone)]
struct ManagedBrowserSession {
    owner_session_id: Option<String>,
    current_url: String,
    _created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BrowserHealthSummary {
    pub enabled: bool,
    pub runnable: bool,
    pub tools_registered: bool,
    pub sidecar_found: bool,
    pub browser_found: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSidecarInstallResult {
    pub version: String,
    pub asset_name: String,
    pub installed_path: String,
    pub downloaded_bytes: u64,
    pub status: BrowserStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSmokeTestResult {
    pub ok: bool,
    pub status: BrowserStatus,
    pub url: String,
    pub final_url: String,
    pub title: String,
    pub load_state: String,
    pub element_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    pub closed: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Clone)]
pub struct BrowserSubsystem {
    config: BrowserConfig,
    status: Arc<RwLock<BrowserStatus>>,
    tools_registered: Arc<AtomicBool>,
    client: Arc<Mutex<Option<BrowserSidecarClient>>>,
    sessions: Arc<RwLock<HashMap<String, ManagedBrowserSession>>>,
    artifact_root: PathBuf,
}

#[derive(Clone, Copy)]
enum BrowserToolKind {
    Status,
    Open,
    Navigate,
    Snapshot,
    Click,
    Type,
    Press,
    Wait,
    Extract,
    Screenshot,
    Close,
}

#[derive(Clone)]
pub struct BrowserTool {
    kind: BrowserToolKind,
    browser: BrowserSubsystem,
    state: Option<AppState>,
}

#[derive(Debug, Deserialize)]
struct BrowserTypeToolArgs {
    session_id: String,
    #[serde(default)]
    element_id: Option<String>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    secret_ref: Option<String>,
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    submit: bool,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct BrowserWaitConditionArgs {
    #[serde(default, alias = "type")]
    kind: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserWaitToolArgs {
    #[serde(alias = "sessionId")]
    session_id: String,
    #[serde(default, alias = "wait_for", alias = "waitFor")]
    condition: Option<BrowserWaitConditionArgs>,
    #[serde(default, alias = "timeoutMs")]
    timeout_ms: Option<u64>,
    #[serde(default, alias = "type")]
    kind: Option<String>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserToolContext {
    #[serde(default, rename = "__session_id")]
    model_session_id: Option<String>,
}

impl BrowserSidecarClient {
    async fn spawn(config: &BrowserConfig) -> anyhow::Result<Self> {
        let sidecar_path = detect_sidecar_binary_path(config.sidecar_path.as_deref())
            .ok_or_else(|| anyhow!("browser_sidecar_not_found"))?;
        let mut cmd = Command::new(&sidecar_path);
        cmd.arg("serve")
            .arg("--transport")
            .arg("stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(path) = config
            .executable_path
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            cmd.env("TANDEM_BROWSER_EXECUTABLE", path);
        }
        if let Some(path) = config
            .user_data_root
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            cmd.env("TANDEM_BROWSER_USER_DATA_ROOT", path);
        }
        cmd.env(
            "TANDEM_BROWSER_ALLOW_NO_SANDBOX",
            bool_env_value(config.allow_no_sandbox),
        );
        cmd.env(
            "TANDEM_BROWSER_HEADLESS",
            bool_env_value(config.headless_default),
        );

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "failed to spawn tandem-browser sidecar at `{}`",
                sidecar_path.display()
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("browser sidecar stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("browser sidecar stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("browser sidecar stderr unavailable"))?;
        let mut client = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
            stderr: BufReader::new(stderr),
            next_id: 1,
        };
        let version: Value = client.call_raw("browser.version", json!({})).await?;
        let protocol = version
            .get("protocol_version")
            .and_then(Value::as_str)
            .unwrap_or("");
        if protocol != BROWSER_PROTOCOL_VERSION {
            anyhow::bail!(
                "protocol_mismatch: expected browser protocol {}, got {}",
                BROWSER_PROTOCOL_VERSION,
                protocol
            );
        }
        Ok(client)
    }

    async fn call_raw(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let request = BrowserRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: json!(id),
            method: method.to_string(),
            params,
        };
        let raw = serde_json::to_string(&request)?;
        self.stdin.write_all(raw.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;

        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).await?;
        if read == 0 {
            let mut stderr = String::new();
            let _ = self.stderr.read_to_string(&mut stderr).await;
            let stderr = stderr.trim();
            if stderr.is_empty() {
                anyhow::bail!("browser sidecar closed the stdio connection");
            }
            anyhow::bail!(
                "browser sidecar closed the stdio connection: {}",
                smoke_excerpt(stderr, 600)
            );
        }
        let response: BrowserRpcResponse =
            serde_json::from_str(line.trim()).context("invalid browser sidecar response")?;
        if let Some(error) = response.error {
            anyhow::bail!("{}", error.message);
        }
        response
            .result
            .ok_or_else(|| anyhow!("browser sidecar returned an empty result"))
    }

    async fn call<T: Serialize, R: for<'de> Deserialize<'de>>(
        &mut self,
        method: &str,
        params: T,
    ) -> anyhow::Result<R> {
        let value = self.call_raw(method, serde_json::to_value(params)?).await?;
        serde_json::from_value(value).context("invalid browser sidecar payload")
    }

    async fn call_value<R: for<'de> Deserialize<'de>>(
        &mut self,
        method: &str,
        params: Value,
    ) -> anyhow::Result<R> {
        let value = self.call_raw(method, params).await?;
        serde_json::from_value(value).context("invalid browser sidecar payload")
    }
}

impl BrowserSubsystem {
    pub fn new(config: BrowserConfig) -> Self {
        let artifact_root = resolve_shared_paths()
            .map(|paths| paths.canonical_root.join("browser-artifacts"))
            .unwrap_or_else(|_| PathBuf::from(".tandem").join("browser-artifacts"));
        Self {
            config,
            status: Arc::new(RwLock::new(BrowserStatus::default())),
            tools_registered: Arc::new(AtomicBool::new(false)),
            client: Arc::new(Mutex::new(None)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            artifact_root,
        }
    }

    pub fn config(&self) -> &BrowserConfig {
        &self.config
    }

    pub async fn install_sidecar(&self) -> anyhow::Result<BrowserSidecarInstallResult> {
        let mut result = install_browser_sidecar(&self.config).await?;
        result.status = self.refresh_status().await;
        Ok(result)
    }

    pub async fn smoke_test(&self, url: Option<String>) -> anyhow::Result<BrowserSmokeTestResult> {
        let status = self.status_snapshot().await;
        if !status.runnable {
            anyhow::bail!(
                "browser_not_runnable: run browser doctor first; current status is not runnable"
            );
        }

        let target_url = url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "https://example.com".to_string());
        let request = BrowserOpenRequest {
            url: target_url.clone(),
            profile_id: None,
            headless: Some(self.config.headless_default),
            viewport: Some(BrowserViewport {
                width: self.config.default_viewport.width,
                height: self.config.default_viewport.height,
            }),
            wait_until: Some("navigation".to_string()),
            executable_path: self.config.executable_path.clone(),
            user_data_root: self.config.user_data_root.clone(),
            allow_no_sandbox: self.config.allow_no_sandbox,
            headless_default: self.config.headless_default,
        };
        let opened: BrowserOpenResult = self.call_sidecar("browser.open", request).await?;
        let session_id = opened.session_id.clone();

        let result = async {
            let snapshot: BrowserSnapshotResult = self
                .call_sidecar(
                    "browser.snapshot",
                    BrowserSnapshotParams {
                        session_id: session_id.clone(),
                        max_elements: Some(25),
                        include_screenshot: false,
                    },
                )
                .await?;
            let extract: BrowserExtractResult = self
                .call_sidecar(
                    "browser.extract",
                    BrowserExtractParams {
                        session_id: session_id.clone(),
                        format: "visible_text".to_string(),
                        max_bytes: Some(4_000),
                    },
                )
                .await?;
            Ok::<BrowserSmokeTestResult, anyhow::Error>(BrowserSmokeTestResult {
                ok: true,
                status,
                url: target_url,
                final_url: snapshot.url,
                title: snapshot.title,
                load_state: snapshot.load_state,
                element_count: snapshot.elements.len(),
                excerpt: Some(smoke_excerpt(&extract.content, 400)),
                closed: false,
            })
        }
        .await;

        let close_result: BrowserCloseResult = self
            .call_sidecar(
                "browser.close",
                BrowserCloseParams {
                    session_id: session_id.clone(),
                },
            )
            .await
            .unwrap_or(BrowserCloseResult {
                session_id,
                closed: false,
            });

        let mut smoke = result?;
        smoke.closed = close_result.closed;
        Ok(smoke)
    }

    pub async fn refresh_status(&self) -> BrowserStatus {
        let config = self.config.clone();
        let evaluated = tokio::task::spawn_blocking(move || evaluate_browser_status(config))
            .await
            .unwrap_or_else(|err| BrowserStatus {
                enabled: false,
                runnable: false,
                headless_default: true,
                sidecar: Default::default(),
                browser: Default::default(),
                blocking_issues: vec![BrowserBlockingIssue {
                    code: "browser_launch_failed".to_string(),
                    message: format!("browser readiness task failed: {}", err),
                }],
                recommendations: vec![
                    "Run `tandem-engine browser doctor --json` on the same host.".to_string(),
                ],
                install_hints: Vec::new(),
                last_checked_at_ms: Some(now_ms()),
                last_error: Some(err.to_string()),
            });
        *self.status.write().await = evaluated.clone();
        evaluated
    }

    pub async fn status_snapshot(&self) -> BrowserStatus {
        let current = self.status.read().await.clone();
        if current
            .last_checked_at_ms
            .is_some_and(|ts| now_ms().saturating_sub(ts) <= STATUS_CACHE_MAX_AGE_MS)
        {
            current
        } else {
            self.refresh_status().await
        }
    }

    pub async fn health_summary(&self) -> BrowserHealthSummary {
        let status = self.status.read().await.clone();
        BrowserHealthSummary {
            enabled: status.enabled,
            runnable: status.runnable,
            tools_registered: self.tools_registered.load(Ordering::Relaxed),
            sidecar_found: status.sidecar.found,
            browser_found: status.browser.found,
            browser_version: status.browser.version,
            last_checked_at_ms: status.last_checked_at_ms,
            last_error: status.last_error,
        }
    }

    pub fn set_tools_registered(&self, value: bool) {
        self.tools_registered.store(value, Ordering::Relaxed);
    }

    pub async fn register_tools(
        &self,
        tools: &ToolRegistry,
        state: Option<AppState>,
    ) -> anyhow::Result<()> {
        tools.unregister_by_prefix("browser_").await;
        tools
            .register_tool(
                "browser_status".to_string(),
                Arc::new(BrowserTool::new(
                    BrowserToolKind::Status,
                    self.clone(),
                    state.clone(),
                )),
            )
            .await;

        let status = self.status_snapshot().await;
        if !status.enabled || !status.runnable {
            self.set_tools_registered(false);
            return Ok(());
        }

        for (name, kind) in [
            ("browser_open", BrowserToolKind::Open),
            ("browser_navigate", BrowserToolKind::Navigate),
            ("browser_snapshot", BrowserToolKind::Snapshot),
            ("browser_click", BrowserToolKind::Click),
            ("browser_type", BrowserToolKind::Type),
            ("browser_press", BrowserToolKind::Press),
            ("browser_wait", BrowserToolKind::Wait),
            ("browser_extract", BrowserToolKind::Extract),
            ("browser_screenshot", BrowserToolKind::Screenshot),
            ("browser_close", BrowserToolKind::Close),
        ] {
            tools
                .register_tool(
                    name.to_string(),
                    Arc::new(BrowserTool::new(kind, self.clone(), state.clone())),
                )
                .await;
        }
        self.set_tools_registered(true);
        Ok(())
    }

    async fn update_last_error(&self, message: impl Into<String>) {
        let mut status = self.status.write().await;
        status.last_error = Some(message.into());
        status.last_checked_at_ms = Some(now_ms());
    }

    async fn call_sidecar<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: T,
    ) -> anyhow::Result<R> {
        let params = serde_json::to_value(params)?;
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(BrowserSidecarClient::spawn(&self.config).await?);
        }
        let result = guard
            .as_mut()
            .expect("browser sidecar client initialized")
            .call_value(method, params.clone())
            .await;
        if let Err(err) = &result {
            *guard = None;
            self.update_last_error(err.to_string()).await;
            if err
                .to_string()
                .contains("browser sidecar closed the stdio connection")
            {
                *guard = Some(BrowserSidecarClient::spawn(&self.config).await?);
                return guard
                    .as_mut()
                    .expect("browser sidecar client reinitialized")
                    .call_value(method, params)
                    .await;
            }
        }
        result
    }

    async fn insert_session(
        &self,
        browser_session_id: String,
        owner_session_id: Option<String>,
        current_url: String,
    ) {
        self.sessions.write().await.insert(
            browser_session_id,
            ManagedBrowserSession {
                owner_session_id,
                current_url,
                _created_at_ms: now_ms(),
                updated_at_ms: now_ms(),
            },
        );
    }

    async fn session(&self, browser_session_id: &str) -> Option<ManagedBrowserSession> {
        self.sessions.read().await.get(browser_session_id).cloned()
    }

    async fn update_session_url(
        &self,
        browser_session_id: &str,
        current_url: String,
    ) -> Option<ManagedBrowserSession> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(browser_session_id)?;
        session.current_url = current_url;
        session.updated_at_ms = now_ms();
        Some(session.clone())
    }

    async fn remove_session(&self, browser_session_id: &str) -> Option<ManagedBrowserSession> {
        self.sessions.write().await.remove(browser_session_id)
    }

    pub async fn close_sessions_for_owner(&self, owner_session_id: &str) -> usize {
        let session_ids = self
            .sessions
            .read()
            .await
            .iter()
            .filter_map(|(session_id, session)| {
                (session.owner_session_id.as_deref() == Some(owner_session_id))
                    .then_some(session_id.clone())
            })
            .collect::<Vec<_>>();
        self.close_session_ids(session_ids).await
    }

    pub async fn close_all_sessions(&self) -> usize {
        let session_ids = self
            .sessions
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        self.close_session_ids(session_ids).await
    }

    async fn close_session_ids(&self, session_ids: Vec<String>) -> usize {
        let mut closed = 0usize;
        for session_id in session_ids {
            let _ = self
                .call_sidecar::<_, BrowserCloseResult>(
                    "browser.close",
                    BrowserCloseParams {
                        session_id: session_id.clone(),
                    },
                )
                .await;
            if self.remove_session(&session_id).await.is_some() {
                closed += 1;
            }
        }
        closed
    }
}

impl BrowserTool {
    fn new(kind: BrowserToolKind, browser: BrowserSubsystem, state: Option<AppState>) -> Self {
        Self {
            kind,
            browser,
            state,
        }
    }

    async fn execute_impl(&self, args: Value) -> anyhow::Result<ToolResult> {
        match self.kind {
            BrowserToolKind::Status => self.execute_status().await,
            BrowserToolKind::Open => self.execute_open(args).await,
            BrowserToolKind::Navigate => self.execute_navigate(args).await,
            BrowserToolKind::Snapshot => self.execute_snapshot(args).await,
            BrowserToolKind::Click => self.execute_click(args).await,
            BrowserToolKind::Type => self.execute_type(args).await,
            BrowserToolKind::Press => self.execute_press(args).await,
            BrowserToolKind::Wait => self.execute_wait(args).await,
            BrowserToolKind::Extract => self.execute_extract(args).await,
            BrowserToolKind::Screenshot => self.execute_screenshot(args).await,
            BrowserToolKind::Close => self.execute_close(args).await,
        }
    }

    async fn execute_status(&self) -> anyhow::Result<ToolResult> {
        let status = self.browser.status_snapshot().await;
        ok_tool_result(
            serde_json::to_value(&status)?,
            json!({
                "enabled": status.enabled,
                "runnable": status.runnable,
                "sidecar_found": status.sidecar.found,
                "browser_found": status.browser.found,
            }),
        )
    }

    async fn execute_open(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let mut request: BrowserOpenRequest =
            serde_json::from_value(args.clone()).context("invalid browser_open arguments")?;
        normalize_browser_open_request(&mut request);
        let status = self.browser.status_snapshot().await;
        if !status.runnable {
            return browser_not_runnable_result(&status);
        }
        ensure_allowed_browser_url(
            &request.url,
            &self
                .effective_allowed_hosts(ctx.model_session_id.as_deref())
                .await,
        )?;
        request.executable_path = self.browser.config.executable_path.clone();
        request.user_data_root = self.browser.config.user_data_root.clone();
        request.allow_no_sandbox = self.browser.config.allow_no_sandbox;
        request.headless_default = self.browser.config.headless_default;
        if request.viewport.is_none() {
            request.viewport = Some(BrowserViewport {
                width: self.browser.config.default_viewport.width,
                height: self.browser.config.default_viewport.height,
            });
        }
        let result: BrowserOpenResult = self.browser.call_sidecar("browser.open", request).await?;
        ensure_allowed_browser_url(
            &result.final_url,
            &self
                .effective_allowed_hosts(ctx.model_session_id.as_deref())
                .await,
        )
        .map_err(|err| anyhow!("host_not_allowed: {}", err))?;
        self.browser
            .insert_session(
                result.session_id.clone(),
                ctx.model_session_id.clone(),
                result.final_url.clone(),
            )
            .await;
        ok_tool_result(
            serde_json::to_value(&result)?,
            json!({
                "session_id": result.session_id,
                "url": result.final_url,
                "headless": result.headless,
            }),
        )
    }

    async fn execute_navigate(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: BrowserNavigateParams =
            serde_json::from_value(args.clone()).context("invalid browser_navigate arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        ensure_allowed_browser_url(
            &params.url,
            &self
                .effective_allowed_hosts(session.owner_session_id.as_deref())
                .await,
        )?;
        let result: BrowserNavigateResult = self
            .browser
            .call_sidecar("browser.navigate", params.clone())
            .await?;
        self.enforce_post_navigation(
            &params.session_id,
            &result.final_url,
            session.owner_session_id.as_deref(),
        )
        .await?;
        ok_tool_result(
            serde_json::to_value(&result)?,
            json!({
                "session_id": result.session_id,
                "url": result.final_url,
            }),
        )
    }

    async fn execute_snapshot(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: BrowserSnapshotParams =
            serde_json::from_value(args.clone()).context("invalid browser_snapshot arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        self.ensure_page_read_allowed(session.owner_session_id.as_deref(), &session.current_url)
            .await?;
        let mut result: BrowserSnapshotResult = self
            .browser
            .call_sidecar("browser.snapshot", params.clone())
            .await?;
        self.browser
            .update_session_url(&params.session_id, result.url.clone())
            .await;

        let screenshot_artifact = if let Some(base64) = result.screenshot_base64.take() {
            Some(
                self.store_artifact(
                    ctx.model_session_id.as_deref(),
                    &params.session_id,
                    "screenshot",
                    params
                        .include_screenshot
                        .then_some(SNAPSHOT_SCREENSHOT_LABEL.to_string()),
                    "png",
                    &base64::engine::general_purpose::STANDARD
                        .decode(base64.as_bytes())
                        .context("invalid snapshot screenshot payload")?,
                    Some(json!({
                        "source": "browser_snapshot",
                        "url": result.url,
                    })),
                )
                .await?,
            )
        } else {
            None
        };
        let payload = json!({
            "session_id": result.session_id,
            "url": result.url,
            "title": result.title,
            "load_state": result.load_state,
            "viewport": result.viewport,
            "elements": result.elements,
            "notices": result.notices,
            "screenshot_artifact": screenshot_artifact,
        });
        ok_tool_result(
            payload.clone(),
            json!({
                "session_id": payload.get("session_id"),
                "url": payload.get("url"),
                "element_count": payload.get("elements").and_then(Value::as_array).map(|rows| rows.len()).unwrap_or(0),
            }),
        )
    }

    async fn execute_click(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: tandem_browser::BrowserClickParams =
            serde_json::from_value(args.clone()).context("invalid browser_click arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        self.ensure_action_allowed(session.owner_session_id.as_deref(), &session.current_url)
            .await?;
        let result: BrowserActionResult = self
            .browser
            .call_sidecar("browser.click", params.clone())
            .await?;
        self.update_action_url(
            &params.session_id,
            result.final_url.as_deref(),
            session.owner_session_id.as_deref(),
        )
        .await?;
        ok_tool_result(
            serde_json::to_value(&result)?,
            json!({
                "session_id": result.session_id,
                "success": result.success,
                "url": result.final_url,
            }),
        )
    }

    async fn execute_type(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: BrowserTypeToolArgs =
            serde_json::from_value(args.clone()).context("invalid browser_type arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        self.ensure_action_allowed(session.owner_session_id.as_deref(), &session.current_url)
            .await?;
        let text = resolve_text_input(params.text.clone(), params.secret_ref.clone())?;
        let request = BrowserTypeParams {
            session_id: params.session_id.clone(),
            element_id: params.element_id.clone(),
            selector: params.selector.clone(),
            text,
            replace: params.replace,
            submit: params.submit,
            timeout_ms: params.timeout_ms,
        };
        let result: BrowserActionResult =
            self.browser.call_sidecar("browser.type", request).await?;
        self.update_action_url(
            &params.session_id,
            result.final_url.as_deref(),
            session.owner_session_id.as_deref(),
        )
        .await?;
        ok_tool_result(
            serde_json::to_value(&result)?,
            json!({
                "session_id": result.session_id,
                "success": result.success,
                "used_secret_ref": params.secret_ref.is_some(),
                "url": result.final_url,
            }),
        )
    }

    async fn execute_press(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: BrowserPressParams =
            serde_json::from_value(args.clone()).context("invalid browser_press arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        self.ensure_action_allowed(session.owner_session_id.as_deref(), &session.current_url)
            .await?;
        let result: BrowserActionResult = self
            .browser
            .call_sidecar("browser.press", params.clone())
            .await?;
        self.update_action_url(
            &params.session_id,
            result.final_url.as_deref(),
            session.owner_session_id.as_deref(),
        )
        .await?;
        ok_tool_result(
            serde_json::to_value(&result)?,
            json!({
                "session_id": result.session_id,
                "success": result.success,
                "url": result.final_url,
            }),
        )
    }

    async fn execute_wait(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params = parse_browser_wait_args(&args).context("invalid browser_wait arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        self.ensure_page_read_allowed(session.owner_session_id.as_deref(), &session.current_url)
            .await?;
        let result: BrowserActionResult = self
            .browser
            .call_sidecar("browser.wait", params.clone())
            .await?;
        self.update_action_url(
            &params.session_id,
            result.final_url.as_deref(),
            session.owner_session_id.as_deref(),
        )
        .await?;
        ok_tool_result(
            serde_json::to_value(&result)?,
            json!({
                "session_id": result.session_id,
                "success": result.success,
                "url": result.final_url,
            }),
        )
    }

    async fn execute_extract(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: BrowserExtractParams =
            serde_json::from_value(args.clone()).context("invalid browser_extract arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        self.ensure_page_read_allowed(session.owner_session_id.as_deref(), &session.current_url)
            .await?;
        let result: BrowserExtractResult = self
            .browser
            .call_sidecar("browser.extract", params.clone())
            .await?;
        let bytes = result.content.as_bytes();
        let artifact = if bytes.len() > INLINE_EXTRACT_LIMIT_BYTES {
            Some(
                self.store_artifact(
                    ctx.model_session_id.as_deref(),
                    &params.session_id,
                    "extract",
                    Some(format!("browser extract ({})", result.format)),
                    extension_for_extract_format(&result.format),
                    bytes,
                    Some(json!({
                        "format": result.format,
                        "truncated": result.truncated,
                        "source": "browser_extract",
                    })),
                )
                .await?,
            )
        } else {
            None
        };
        let payload = json!({
            "session_id": result.session_id,
            "format": result.format,
            "content": artifact.is_none().then_some(result.content),
            "truncated": result.truncated,
            "artifact": artifact,
        });
        ok_tool_result(
            payload.clone(),
            json!({
                "session_id": payload.get("session_id"),
                "format": payload.get("format"),
                "artifact": payload.get("artifact").is_some(),
            }),
        )
    }

    async fn execute_screenshot(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: BrowserScreenshotParams =
            serde_json::from_value(args.clone()).context("invalid browser_screenshot arguments")?;
        let session = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        self.ensure_page_read_allowed(session.owner_session_id.as_deref(), &session.current_url)
            .await?;
        let result: BrowserScreenshotResult = self
            .browser
            .call_sidecar("browser.screenshot", params.clone())
            .await?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result.data_base64.as_bytes())
            .context("invalid screenshot payload")?;
        let artifact = self
            .store_artifact(
                ctx.model_session_id.as_deref(),
                &params.session_id,
                "screenshot",
                result.label.clone(),
                "png",
                &bytes,
                Some(json!({
                    "mime_type": result.mime_type,
                    "bytes": result.bytes,
                    "source": "browser_screenshot",
                })),
            )
            .await?;
        ok_tool_result(
            json!({
                "session_id": result.session_id,
                "artifact": artifact,
                "summary": format!("Saved screenshot artifact ({} bytes).", result.bytes),
            }),
            json!({
                "session_id": result.session_id,
                "artifact_id": artifact.artifact_id,
            }),
        )
    }

    async fn execute_close(&self, args: Value) -> anyhow::Result<ToolResult> {
        let ctx = parse_tool_context(&args);
        let params: BrowserCloseParams =
            serde_json::from_value(args.clone()).context("invalid browser_close arguments")?;
        let _ = self
            .load_session(&params.session_id, ctx.model_session_id.as_deref())
            .await?;
        let result: BrowserCloseResult = self
            .browser
            .call_sidecar("browser.close", params.clone())
            .await?;
        self.browser.remove_session(&params.session_id).await;
        ok_tool_result(
            serde_json::to_value(&result)?,
            json!({
                "session_id": result.session_id,
                "closed": result.closed,
            }),
        )
    }

    async fn load_session(
        &self,
        browser_session_id: &str,
        model_session_id: Option<&str>,
    ) -> anyhow::Result<ManagedBrowserSession> {
        let session = self
            .browser
            .session(browser_session_id)
            .await
            .ok_or_else(|| anyhow!("session `{}` not found", browser_session_id))?;
        if let (Some(owner), Some(model_session_id)) =
            (session.owner_session_id.as_deref(), model_session_id)
        {
            if owner != model_session_id {
                anyhow::bail!(
                    "browser session `{}` belongs to a different engine session",
                    browser_session_id
                );
            }
        }
        Ok(session)
    }

    async fn effective_allowed_hosts(&self, model_session_id: Option<&str>) -> Vec<String> {
        if let Some(model_session_id) = model_session_id {
            if let Some(state) = self.state.as_ref() {
                if let Some(instance) = state
                    .agent_teams
                    .instance_for_session(model_session_id)
                    .await
                {
                    if !instance.capabilities.net_scopes.allow_hosts.is_empty() {
                        return normalize_allowed_hosts(
                            instance.capabilities.net_scopes.allow_hosts,
                        );
                    }
                }
            }
        }
        normalize_allowed_hosts(self.browser.config.allowed_hosts.clone())
    }

    async fn ensure_page_read_allowed(
        &self,
        model_session_id: Option<&str>,
        current_url: &str,
    ) -> anyhow::Result<()> {
        ensure_allowed_browser_url(
            current_url,
            &self.effective_allowed_hosts(model_session_id).await,
        )?;
        Ok(())
    }

    async fn ensure_action_allowed(
        &self,
        model_session_id: Option<&str>,
        current_url: &str,
    ) -> anyhow::Result<()> {
        self.ensure_page_read_allowed(model_session_id, current_url)
            .await?;
        let host = browser_url_host(current_url)?;
        if !is_local_or_private_host(&host)
            && !self.external_integrations_allowed(model_session_id).await
        {
            anyhow::bail!(
                "external integrations are disabled for this routine session on host `{}`",
                host
            );
        }
        Ok(())
    }

    async fn external_integrations_allowed(&self, model_session_id: Option<&str>) -> bool {
        let Some(model_session_id) = model_session_id else {
            return true;
        };
        let Some(state) = self.state.as_ref() else {
            return true;
        };
        let Some(policy) = state.routine_session_policy(model_session_id).await else {
            return true;
        };
        state
            .get_routine(&policy.routine_id)
            .await
            .map(|routine| routine.external_integrations_allowed)
            .unwrap_or(true)
    }

    async fn enforce_post_navigation(
        &self,
        browser_session_id: &str,
        final_url: &str,
        model_session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Err(err) = ensure_allowed_browser_url(
            final_url,
            &self.effective_allowed_hosts(model_session_id).await,
        ) {
            let _ = self
                .browser
                .call_sidecar::<_, BrowserCloseResult>(
                    "browser.close",
                    BrowserCloseParams {
                        session_id: browser_session_id.to_string(),
                    },
                )
                .await;
            self.browser.remove_session(browser_session_id).await;
            return Err(anyhow!("host_not_allowed: {}", err));
        }
        self.browser
            .update_session_url(browser_session_id, final_url.to_string())
            .await;
        Ok(())
    }

    async fn update_action_url(
        &self,
        browser_session_id: &str,
        final_url: Option<&str>,
        model_session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(final_url) = final_url {
            self.enforce_post_navigation(browser_session_id, final_url, model_session_id)
                .await?;
        }
        Ok(())
    }

    async fn store_artifact(
        &self,
        model_session_id: Option<&str>,
        browser_session_id: &str,
        kind: &str,
        label: Option<String>,
        extension: &str,
        bytes: &[u8],
        metadata: Option<Value>,
    ) -> anyhow::Result<BrowserArtifactRef> {
        fs::create_dir_all(&self.browser.artifact_root).await?;
        let artifact_id = format!("artifact-{}", Uuid::new_v4());
        let file_name = format!("{artifact_id}.{extension}");
        let target = self.browser.artifact_root.join(file_name);
        fs::write(&target, bytes)
            .await
            .with_context(|| format!("failed to write browser artifact `{}`", target.display()))?;
        let artifact = BrowserArtifactRef {
            artifact_id: artifact_id.clone(),
            uri: target.to_string_lossy().to_string(),
            kind: kind.to_string(),
            label,
            created_at_ms: now_ms(),
            metadata,
        };
        self.append_routine_artifact_if_needed(
            model_session_id,
            artifact.clone(),
            browser_session_id,
        )
        .await;
        Ok(artifact)
    }

    async fn append_routine_artifact_if_needed(
        &self,
        model_session_id: Option<&str>,
        artifact: BrowserArtifactRef,
        browser_session_id: &str,
    ) {
        let Some(model_session_id) = model_session_id else {
            return;
        };
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Some(policy) = state.routine_session_policy(model_session_id).await else {
            return;
        };
        let run_artifact = RoutineRunArtifact {
            artifact_id: artifact.artifact_id.clone(),
            uri: artifact.uri.clone(),
            kind: artifact.kind.clone(),
            label: artifact.label.clone(),
            created_at_ms: artifact.created_at_ms,
            metadata: artifact.metadata.clone(),
        };
        let _ = state
            .append_routine_run_artifact(&policy.run_id, run_artifact.clone())
            .await;
        state.event_bus.publish(EngineEvent::new(
            "routine.run.artifact_added",
            json!({
                "runID": policy.run_id,
                "routineID": policy.routine_id,
                "browserSessionID": browser_session_id,
                "artifact": run_artifact,
            }),
        ));
    }
}
