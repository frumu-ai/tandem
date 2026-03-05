use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_browser::{
    detect_sidecar_binary_path, run_doctor, BrowserActionResult, BrowserArtifactRef,
    BrowserBlockingIssue, BrowserCloseParams, BrowserCloseResult, BrowserDoctorOptions,
    BrowserExtractParams, BrowserExtractResult, BrowserNavigateParams, BrowserNavigateResult,
    BrowserOpenRequest, BrowserOpenResult, BrowserPressParams, BrowserRpcRequest,
    BrowserRpcResponse, BrowserScreenshotParams, BrowserScreenshotResult, BrowserSnapshotParams,
    BrowserSnapshotResult, BrowserStatus, BrowserTypeParams, BrowserViewport, BrowserWaitParams,
    BROWSER_PROTOCOL_VERSION,
};
use tandem_core::{resolve_shared_paths, BrowserConfig};
use tandem_tools::{Tool, ToolRegistry};
use tandem_types::{EngineEvent, ToolResult, ToolSchema};
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{now_ms, AppState, RoutineRunArtifact, RuntimeState};

const STATUS_CACHE_MAX_AGE_MS: u64 = 30_000;
const INLINE_EXTRACT_LIMIT_BYTES: usize = 24_000;
const SNAPSHOT_SCREENSHOT_LABEL: &str = "browser snapshot";

#[derive(Debug)]
struct BrowserSidecarClient {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
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
            .stderr(Stdio::null());
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
            if config.allow_no_sandbox { "1" } else { "0" },
        );
        cmd.env(
            "TANDEM_BROWSER_HEADLESS",
            if config.headless_default { "1" } else { "0" },
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
        let mut client = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
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
            anyhow::bail!("browser sidecar closed the stdio connection");
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
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            *guard = Some(BrowserSidecarClient::spawn(&self.config).await?);
        }
        let result = guard
            .as_mut()
            .expect("browser sidecar client initialized")
            .call(method, params)
            .await;
        if let Err(err) = &result {
            *guard = None;
            self.update_last_error(err.to_string()).await;
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
        let params: BrowserWaitParams =
            serde_json::from_value(args.clone()).context("invalid browser_wait arguments")?;
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
        BrowserToolKind::Status => ToolSchema {
            name: "browser_status".to_string(),
            description:
                "Check browser automation readiness and install guidance. Call this first when browser tools may be unavailable."
                    .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        BrowserToolKind::Open => ToolSchema {
            name: "browser_open".to_string(),
            description:
                "Open a URL in a browser session. Only http/https are allowed. Omit profile_id for an ephemeral session."
                    .to_string(),
            input_schema: json!({
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
        },
        BrowserToolKind::Navigate => ToolSchema {
            name: "browser_navigate".to_string(),
            description: "Navigate an existing browser session to a new URL.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "url": { "type": "string" },
                    "wait_until": { "type": "string", "enum": ["navigation", "network_idle"] }
                },
                "required": ["session_id", "url"]
            }),
        },
        BrowserToolKind::Snapshot => ToolSchema {
            name: "browser_snapshot".to_string(),
            description:
                "Capture a bounded page summary with stable element_id values. Call this before click/type on a new page or after navigation."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "max_elements": { "type": "integer", "minimum": 1, "maximum": 200 },
                    "include_screenshot": { "type": "boolean" }
                },
                "required": ["session_id"]
            }),
        },
        BrowserToolKind::Click => ToolSchema {
            name: "browser_click".to_string(),
            description:
                "Click a visible page element by element_id when possible. Use wait_for to make navigation and selector waits race-free."
                    .to_string(),
            input_schema: json!({
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
        },
        BrowserToolKind::Type => ToolSchema {
            name: "browser_type".to_string(),
            description:
                "Type text into an element. Prefer secret_ref over text for credentials; secret_ref resolves from the host environment and is redacted from logs."
                    .to_string(),
            input_schema: json!({
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
        },
        BrowserToolKind::Press => ToolSchema {
            name: "browser_press".to_string(),
            description: "Dispatch a key press in the active page context.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "key": { "type": "string" },
                    "wait_for": wait_condition_schema(),
                    "timeout_ms": { "type": "integer", "minimum": 250, "maximum": 120000 }
                },
                "required": ["session_id", "key"]
            }),
        },
        BrowserToolKind::Wait => ToolSchema {
            name: "browser_wait".to_string(),
            description: "Wait for a selector, text, URL fragment, navigation, or network idle.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "condition": wait_condition_schema(),
                    "timeout_ms": { "type": "integer", "minimum": 250, "maximum": 120000 }
                },
                "required": ["session_id", "condition"]
            }),
        },
        BrowserToolKind::Extract => ToolSchema {
            name: "browser_extract".to_string(),
            description:
                "Extract page content as visible_text, markdown, or html. Prefer this over screenshots when you need text."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "format": { "type": "string", "enum": ["visible_text", "markdown", "html"] },
                    "max_bytes": { "type": "integer", "minimum": 1024, "maximum": 2000000 }
                },
                "required": ["session_id", "format"]
            }),
        },
        BrowserToolKind::Screenshot => ToolSchema {
            name: "browser_screenshot".to_string(),
            description: "Capture a screenshot and store it as a browser artifact.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "full_page": { "type": "boolean" },
                    "label": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        },
        BrowserToolKind::Close => ToolSchema {
            name: "browser_close".to_string(),
            description: "Close a browser session and release its resources.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_core::BrowserConfig;
    use tandem_tools::ToolRegistry;

    #[test]
    fn local_and_private_hosts_are_detected() {
        assert!(is_local_or_private_host("localhost"));
        assert!(is_local_or_private_host("127.0.0.1"));
        assert!(is_local_or_private_host("10.1.2.3"));
        assert!(is_local_or_private_host("192.168.0.10"));
        assert!(!is_local_or_private_host("example.com"));
        assert!(!is_local_or_private_host("8.8.8.8"));
    }

    #[test]
    fn allow_host_check_accepts_subdomains() {
        let allow_hosts = vec!["example.com".to_string()];
        ensure_allowed_browser_url("https://example.com/path", &allow_hosts).expect("root host");
        ensure_allowed_browser_url("https://app.example.com/path", &allow_hosts)
            .expect("subdomain host");
        let err =
            ensure_allowed_browser_url("https://example.org/path", &allow_hosts).expect_err("deny");
        assert!(err.to_string().contains("allowlist"));
    }

    #[tokio::test]
    async fn register_tools_keeps_browser_status_available_when_disabled() {
        let tools = ToolRegistry::new();
        let browser = BrowserSubsystem::new(BrowserConfig::default());

        browser
            .register_tools(&tools, None)
            .await
            .expect("register browser tools");

        let names = tools
            .list()
            .await
            .into_iter()
            .map(|schema| schema.name)
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "browser_status"));
        assert!(!names.iter().any(|name| name == "browser_open"));
        assert!(!browser.health_summary().await.tools_registered);
    }

    #[tokio::test]
    async fn close_sessions_for_owner_removes_matching_sessions() {
        let browser = BrowserSubsystem::new(BrowserConfig::default());
        browser
            .insert_session(
                "session-1".to_string(),
                Some("owner-1".to_string()),
                "https://example.com".to_string(),
            )
            .await;
        browser
            .insert_session(
                "session-2".to_string(),
                Some("owner-2".to_string()),
                "https://example.org".to_string(),
            )
            .await;

        let closed = browser.close_sessions_for_owner("owner-1").await;

        assert_eq!(closed, 1);
        assert!(browser.session("session-1").await.is_none());
        assert!(browser.session("session-2").await.is_some());
    }
}
