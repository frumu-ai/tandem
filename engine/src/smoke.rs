// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! `tandem-engine smoke` (TAN-227): one-command end-to-end health check for
//! the governed runtime path. Runs deterministic scenarios against either a
//! fresh in-process server (no network, no API keys — the local echo
//! provider is used) or an existing engine via `--against`.
//!
//! Scenarios:
//! - `session-prompt`: session create → prompt round-trip → assistant reply.
//! - `approval-gate`: automation run → `awaiting_approval` → approve via the
//!   gate API → run completes with gate history recorded.
//! - `policy-denial`: agent-sourced mutation is denied with a structured
//!   error and (in-process) a protected audit event.
//! - `memory-roundtrip`: governed memory put → search returns the fact.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::{json, Value};
use tandem_server::AppState;
use uuid::Uuid;

pub(crate) struct SmokeOptions {
    /// Base URL of an already-running engine. `None` boots an isolated
    /// in-process server on a loopback port with a fresh state dir.
    pub against: Option<String>,
    /// Bearer token for `--against` mode.
    pub token: Option<String>,
    /// Scenario name filter; empty runs everything.
    pub scenarios: Vec<String>,
    pub json: bool,
    pub timeout_secs: u64,
}

struct SmokeContext {
    client: reqwest::Client,
    base_url: String,
    token: Option<String>,
    /// Set in in-process mode: the isolated state dir (used for audit-file
    /// assertions). Remote targets skip file-level checks.
    state_dir: Option<PathBuf>,
    in_process: bool,
    deadline: Instant,
}

struct ScenarioResult {
    name: &'static str,
    passed: bool,
    duration_ms: u128,
    detail: String,
}

/// Operator provenance for governed mutations: in the OSS engine only human
/// actors may create/run automations, and `control_panel` is the canonical
/// human request source.
const OPERATOR_HEADERS: &[(&str, &str)] = &[("x-tandem-request-source", "control_panel")];

const ALL_SCENARIOS: &[&str] = &[
    "session-prompt",
    "approval-gate",
    "policy-denial",
    "memory-roundtrip",
];

pub(crate) async fn run_smoke(options: SmokeOptions) -> anyhow::Result<bool> {
    for requested in &options.scenarios {
        if !ALL_SCENARIOS.contains(&requested.as_str()) {
            anyhow::bail!(
                "unknown scenario `{requested}`; available: {}",
                ALL_SCENARIOS.join(", ")
            );
        }
    }

    let deadline = Instant::now() + Duration::from_secs(options.timeout_secs);
    // Keep the temp dir alive for the whole run in in-process mode.
    let mut _state_dir_guard: Option<tempfile::TempDir> = None;

    let ctx = match &options.against {
        Some(base_url) => SmokeContext {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            token: options.token.clone(),
            state_dir: None,
            in_process: false,
            deadline,
        },
        None => {
            let (ctx, guard) = boot_in_process(deadline).await?;
            _state_dir_guard = Some(guard);
            ctx
        }
    };

    wait_for_ready(&ctx).await?;

    let selected: Vec<&'static str> = ALL_SCENARIOS
        .iter()
        .copied()
        .filter(|name| options.scenarios.is_empty() || options.scenarios.iter().any(|s| s == name))
        .collect();

    let mut results = Vec::new();
    for name in selected {
        let started = Instant::now();
        let outcome = match name {
            "session-prompt" => scenario_session_prompt(&ctx).await,
            "approval-gate" => scenario_approval_gate(&ctx).await,
            "policy-denial" => scenario_policy_denial(&ctx).await,
            "memory-roundtrip" => scenario_memory_roundtrip(&ctx).await,
            _ => unreachable!(),
        };
        results.push(ScenarioResult {
            name,
            passed: outcome.is_ok(),
            duration_ms: started.elapsed().as_millis(),
            detail: match outcome {
                Ok(detail) => detail,
                Err(error) => format!("{error:#}"),
            },
        });
    }

    let all_passed = results.iter().all(|result| result.passed);
    if options.json {
        let payload = json!({
            "ok": all_passed,
            "target": if ctx.in_process { "in-process".to_string() } else { ctx.base_url.clone() },
            "scenarios": results.iter().map(|result| json!({
                "name": result.name,
                "status": if result.passed { "pass" } else { "fail" },
                "duration_ms": result.duration_ms,
                "detail": result.detail,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!(
            "Tandem runtime smoke test — target: {}",
            if ctx.in_process {
                "in-process (fresh state, local echo provider)"
            } else {
                ctx.base_url.as_str()
            }
        );
        println!("{:<18} {:<6} {:>9}  detail", "scenario", "result", "ms");
        for result in &results {
            println!(
                "{:<18} {:<6} {:>9}  {}",
                result.name,
                if result.passed { "PASS" } else { "FAIL" },
                result.duration_ms,
                result.detail
            );
        }
        println!(
            "{}",
            if all_passed {
                "All scenarios passed."
            } else {
                "One or more scenarios FAILED."
            }
        );
    }
    Ok(all_passed)
}

async fn boot_in_process(deadline: Instant) -> anyhow::Result<(SmokeContext, tempfile::TempDir)> {
    let temp = tempfile::TempDir::new().context("create smoke state dir")?;
    let state_dir = temp.path().join("state");
    let home_dir = temp.path().join("home");
    std::fs::create_dir_all(&state_dir)?;
    std::fs::create_dir_all(&home_dir)?;

    // Isolate from any host configuration: a fresh config means no providers
    // are configured, so the registry falls back to the deterministic local
    // echo provider, and all state/audit paths land in the temp dir.
    std::env::set_var("TANDEM_STATE_DIR", &state_dir);
    std::env::set_var("TANDEM_HOME", &home_dir);
    std::env::set_var(
        "TANDEM_GLOBAL_CONFIG",
        temp.path().join("global-config.json"),
    );
    // Embedding models would download weights; the smoke must run offline.
    std::env::set_var("TANDEM_DISABLE_EMBEDDINGS", "1");

    let token = Uuid::new_v4().to_string();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").context("pick smoke port")?;
        listener.local_addr()?.port()
    };
    let addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let base_url = format!("http://127.0.0.1:{port}");

    let state = AppState::new_starting(Uuid::new_v4().to_string(), true);
    state.set_api_token(Some(token.clone())).await;
    state.set_server_base_url(base_url.clone());

    let init_state = state.clone();
    let init_dir = state_dir.clone();
    tokio::spawn(async move {
        if let Err(error) =
            crate::initialize_runtime(init_state.clone(), init_dir, None, None).await
        {
            init_state
                .mark_failed("runtime_init", error.to_string())
                .await;
        }
    });
    tokio::spawn(async move {
        if let Err(error) = tandem_server::serve(addr, state).await {
            eprintln!("smoke server exited: {error:#}");
        }
    });

    Ok((
        SmokeContext {
            client: reqwest::Client::new(),
            base_url,
            token: Some(token),
            state_dir: Some(state_dir),
            in_process: true,
            deadline,
        },
        temp,
    ))
}

impl SmokeContext {
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    async fn get_json(&self, path: &str) -> anyhow::Result<(u16, Value)> {
        let response = self
            .authorize(self.client.get(self.url(path)))
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        let status = response.status().as_u16();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        Ok((status, body))
    }

    async fn post_json(
        &self,
        path: &str,
        body: Value,
        headers: &[(&str, &str)],
    ) -> anyhow::Result<(u16, Value)> {
        let mut request = self.authorize(self.client.post(self.url(path))).json(&body);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        let status = response.status().as_u16();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        Ok((status, body))
    }

    async fn poll_until<F>(&self, what: &str, mut check: F) -> anyhow::Result<Value>
    where
        F: AsyncFnMut() -> anyhow::Result<Option<Value>>,
    {
        loop {
            if let Some(value) = check().await? {
                return Ok(value);
            }
            if Instant::now() >= self.deadline {
                anyhow::bail!("timed out waiting for {what}");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

async fn wait_for_ready(ctx: &SmokeContext) -> anyhow::Result<()> {
    ctx.poll_until("engine readiness (/global/health)", async || {
        match ctx.get_json("/global/health").await {
            Ok((200, body))
                if body["ready"].as_bool() == Some(true)
                    || body["phase"].as_str() == Some("ready") =>
            {
                Ok(Some(body))
            }
            Ok(_) | Err(_) => Ok(None),
        }
    })
    .await
    .map(|_| ())
}

/// Session create → prompt → assistant reply observed via the message log.
async fn scenario_session_prompt(ctx: &SmokeContext) -> anyhow::Result<String> {
    let (status, body) = ctx
        .post_json("/session", json!({ "title": "smoke session" }), &[])
        .await?;
    anyhow::ensure!(status == 200, "session create returned {status}: {body}");
    let session_id = body["id"]
        .as_str()
        .context("session create response missing `id`")?
        .to_string();

    let mut prompt = json!({
        "parts": [{ "type": "text", "text": "tandem smoke ping" }],
    });
    if ctx.in_process {
        // Fresh state has no configured providers, so the deterministic
        // local echo provider is the registry's default and only entry.
        prompt["model"] = json!({ "provider_id": "local", "model_id": "echo-1" });
    }
    let (status, body) = ctx
        .post_json(&format!("/session/{session_id}/prompt_async"), prompt, &[])
        .await?;
    anyhow::ensure!(
        (200..300).contains(&status),
        "prompt_async returned {status}: {body}"
    );

    let reply = ctx
        .poll_until("assistant reply in session message log", async || {
            let (status, body) = ctx
                .get_json(&format!("/session/{session_id}/message"))
                .await?;
            if status != 200 {
                return Ok(None);
            }
            let empty = Vec::new();
            let messages = body.as_array().unwrap_or(&empty);
            for message in messages {
                let role = message["info"]["role"].as_str().unwrap_or_default();
                if role != "assistant" {
                    continue;
                }
                for part in message["parts"].as_array().unwrap_or(&empty) {
                    if let Some(text) = part["text"].as_str() {
                        if !text.trim().is_empty() {
                            return Ok(Some(Value::String(text.to_string())));
                        }
                    }
                }
            }
            Ok(None)
        })
        .await?;
    let reply = reply.as_str().unwrap_or_default().to_string();
    if ctx.in_process {
        anyhow::ensure!(
            reply.contains("tandem smoke ping"),
            "echo provider reply did not round-trip the prompt: {reply:?}"
        );
    }
    Ok(format!("assistant replied ({} chars)", reply.len()))
}

fn smoke_automation_spec(automation_id: &str) -> Value {
    json!({
        "automation_id": automation_id,
        "name": "Smoke approval automation",
        "description": "Synthetic automation exercising the approval gate path",
        "status": "active",
        "schedule": {
            "type": "manual",
            "timezone": "UTC",
            "misfire_policy": { "type": "run_once" }
        },
        "agents": [{
            "agent_id": "smoke-agent",
            "display_name": "Smoke Agent",
            "model_policy": {
                "default_model": { "provider_id": "local", "model_id": "echo-1" }
            },
            "tool_policy": { "allowlist": ["read"], "denylist": [] },
            "mcp_policy": { "allowed_servers": [] }
        }],
        "flow": {
            "nodes": [{
                "node_id": "approval",
                "agent_id": "smoke-agent",
                "objective": "Approve the smoke test run",
                "depends_on": [],
                "input_refs": [],
                "stage_kind": "approval",
                // The plan compiler seeds approval nodes with an
                // `approval_gate` contract; without it the node defaults to a
                // structured_json artifact deliverable and completion repair
                // loops on the gate forever.
                "output_contract": { "kind": "approval_gate" },
                "gate": {
                    "required": true,
                    "decisions": ["approve", "rework", "cancel"],
                    "rework_targets": [],
                    "instructions": "Smoke test approval gate"
                },
                "metadata": {
                    "builder": { "title": "Approval", "prompt": "", "role": "approver" }
                }
            }]
        },
        "execution": { "max_parallel_agents": 1 }
    })
}

/// Submit a gated automation run, observe `awaiting_approval`, approve via
/// the gate API, and verify completion with a recorded gate decision.
async fn scenario_approval_gate(ctx: &SmokeContext) -> anyhow::Result<String> {
    let automation_id = format!("smoke-gate-{}", Uuid::new_v4());
    let (status, body) = ctx
        .post_json(
            "/automations/v2",
            smoke_automation_spec(&automation_id),
            OPERATOR_HEADERS,
        )
        .await?;
    anyhow::ensure!(status == 200, "automation create returned {status}: {body}");

    let (status, body) = ctx
        .post_json(
            &format!("/automations/v2/{automation_id}/run_now"),
            json!({}),
            OPERATOR_HEADERS,
        )
        .await?;
    anyhow::ensure!(status == 200, "run_now returned {status}: {body}");
    let run_id = body["run"]["run_id"]
        .as_str()
        .or_else(|| body["runID"].as_str())
        .context("run_now response missing run id")?
        .to_string();

    let run_path = format!("/automations/v2/runs/{run_id}");
    ctx.poll_until("run to reach awaiting_approval", async || {
        let (status, body) = ctx.get_json(&run_path).await?;
        if status != 200 {
            return Ok(None);
        }
        match run_status(&body) {
            Some("awaiting_approval") => Ok(Some(body)),
            Some("failed") | Some("blocked") | Some("cancelled") => {
                anyhow::bail!("run entered terminal state before the gate: {body}")
            }
            _ => Ok(None),
        }
    })
    .await?;

    let (status, body) = ctx
        .post_json(
            &format!("/automations/v2/runs/{run_id}/gate"),
            json!({ "decision": "approve", "reason": "smoke test approval" }),
            OPERATOR_HEADERS,
        )
        .await?;
    anyhow::ensure!(status == 200, "gate decision returned {status}: {body}");

    let mut last_state = String::new();
    let run = ctx
        .poll_until("run completion after approval", async || {
            let (status, body) = ctx.get_json(&run_path).await?;
            if status != 200 {
                return Ok(None);
            }
            last_state = format!(
                "status={:?} detail={:?} pending={:?} completed={:?} gate_history={} lifecycle={}",
                run_status(&body),
                body["run"]["detail"].as_str(),
                body["run"]["checkpoint"]["pending_nodes"],
                body["run"]["checkpoint"]["completed_nodes"],
                body["run"]["checkpoint"]["gate_history"],
                body["run"]["checkpoint"]["lifecycle_history"],
            );
            match run_status(&body) {
                Some("completed") => Ok(Some(body)),
                Some("failed") | Some("blocked") | Some("cancelled") => {
                    anyhow::bail!("run did not complete after approval: {body}")
                }
                _ => Ok(None),
            }
        })
        .await
        .with_context(|| format!("last run state: {last_state}"))?;

    let gate_history_recorded = ["gate_history", "gateHistory"].iter().any(|key| {
        locate_array(&run, key)
            .map(|entries| !entries.is_empty())
            .unwrap_or(false)
    });
    anyhow::ensure!(
        gate_history_recorded,
        "completed run is missing a recorded gate decision"
    );
    Ok("gate hit, approved via API, run completed with gate history".to_string())
}

fn run_status(body: &Value) -> Option<&str> {
    body["status"]
        .as_str()
        .or_else(|| body["run"]["status"].as_str())
}

/// Find an array under `key` at the top level or one level down (the run
/// record nests some fields under `run`/`checkpoint`).
fn locate_array<'a>(body: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    if let Some(entries) = body[key].as_array() {
        return Some(entries);
    }
    for parent in ["run", "checkpoint"] {
        if let Some(entries) = body[parent][key].as_array() {
            return Some(entries);
        }
        if let Some(entries) = body["run"]["checkpoint"][key].as_array() {
            return Some(entries);
        }
        let _ = parent;
    }
    None
}

/// An agent-sourced governance mutation must be denied with a structured
/// error; in-process, the denial must also land in the protected audit log.
async fn scenario_policy_denial(ctx: &SmokeContext) -> anyhow::Result<String> {
    let automation_id = format!("smoke-deny-{}", Uuid::new_v4());
    let (status, body) = ctx
        .post_json(
            "/automations/v2",
            smoke_automation_spec(&automation_id),
            OPERATOR_HEADERS,
        )
        .await?;
    anyhow::ensure!(status == 200, "automation create returned {status}: {body}");

    let (status, body) = ctx
        .post_json(
            &format!("/automations/v2/{automation_id}/share"),
            json!({ "visibility": "org" }),
            &[
                ("x-tandem-request-source", "agent"),
                ("x-tandem-agent-id", "smoke-denied-agent"),
            ],
        )
        .await?;
    // In the OSS build agent-owned mutation is denied as a premium-feature
    // 501; with premium governance it is a 4xx policy denial. Both must be
    // structured and audited.
    anyhow::ensure!(
        (400..=501).contains(&status),
        "agent-sourced share must be denied, got {status}: {body}"
    );
    anyhow::ensure!(
        body.get("error").is_some() || body.get("code").is_some(),
        "denial must carry a structured error body, got: {body}"
    );

    if let Some(state_dir) = &ctx.state_dir {
        let audit_path = state_dir
            .join("data")
            .join("audit")
            .join("protected_events.log.jsonl");
        let audit = ctx
            .poll_until("protected audit event for denial", async || {
                match tokio::fs::read_to_string(&audit_path).await {
                    Ok(contents) if contents.contains("smoke-denied-agent") => {
                        Ok(Some(Value::Bool(true)))
                    }
                    _ => Ok(None),
                }
            })
            .await;
        anyhow::ensure!(
            audit.is_ok(),
            "denial was not recorded in the protected audit log at {}",
            audit_path.display()
        );
        return Ok("structured denial + protected audit event recorded".to_string());
    }
    Ok("structured denial returned (audit check skipped for remote target)".to_string())
}

/// Governed memory put → search round-trip using a session-tier capability.
async fn scenario_memory_roundtrip(ctx: &SmokeContext) -> anyhow::Result<String> {
    let run_id = format!("smoke-memory-{}", Uuid::new_v4());
    let probe = format!("tandem smoke memory probe {}", Uuid::new_v4());
    let partition = json!({
        "org_id": "local",
        "workspace_id": "local",
        "project_id": "smoke",
        "tier": "session"
    });
    let capability = json!({
        "run_id": run_id,
        "subject": "smoke-operator",
        "org_id": "local",
        "workspace_id": "local",
        "project_id": "smoke",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": [],
            "require_review_for_promote": false,
            "allow_auto_use_tiers": []
        },
        "expires_at": 9_999_999_999_999u64
    });

    let (status, body) = ctx
        .post_json(
            "/memory/put",
            json!({
                "run_id": run_id,
                "partition": partition,
                "kind": "fact",
                "content": probe,
                "classification": "internal",
                "capability": capability
            }),
            &[],
        )
        .await?;
    anyhow::ensure!(status == 200, "memory put returned {status}: {body}");

    let found = ctx
        .poll_until("memory search to return the stored fact", async || {
            let (status, body) = ctx
                .post_json(
                    "/memory/search",
                    json!({
                        "run_id": run_id,
                        "query": "tandem smoke memory probe",
                        "partition": partition,
                        "read_scopes": ["session"],
                        "capability": capability
                    }),
                    &[],
                )
                .await?;
            if status == 200 && body.to_string().contains(&probe) {
                Ok(Some(Value::Bool(true)))
            } else {
                Ok(None)
            }
        })
        .await;
    anyhow::ensure!(found.is_ok(), "stored fact was not returned by search");
    Ok("memory fact stored and retrieved".to_string())
}
