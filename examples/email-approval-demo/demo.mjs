#!/usr/bin/env node

import { createInterface } from "node:readline/promises";
import { stdin as input, stdout as output } from "node:process";
import { spawn } from "node:child_process";
import { createWriteStream, existsSync } from "node:fs";
import { mkdir, readFile, rm, writeFile, copyFile } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const PROVIDER_ENV = [
  "ANTHROPIC_API_KEY",
  "AZURE_OPENAI_API_KEY",
  "BEDROCK_API_KEY",
  "COHERE_API_KEY",
  "GITHUB_TOKEN",
  "GROQ_API_KEY",
  "MISTRAL_API_KEY",
  "OLLAMA_URL",
  "OPENAI_API_KEY",
  "OPENCODE_ZEN_API_KEY",
  "OPENROUTER_API_KEY",
  "TOGETHER_API_KEY",
  "VERTEX_API_KEY",
];

const __filename = fileURLToPath(import.meta.url);
const exampleDir = path.dirname(__filename);
const repoRoot = path.resolve(exampleDir, "..", "..");
const DRAFT_TOOL = "mcp.email_demo.email_draft";
const SEND_TOOL = "mcp.email_demo.email_send";

function parseArgs(argv) {
  const out = {
    nonInteractive: false,
    skipBuild: false,
    keepArtifacts: false,
    decision: "approve",
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--non-interactive") out.nonInteractive = true;
    else if (arg === "--skip-build") out.skipBuild = true;
    else if (arg === "--keep-artifacts") out.keepArtifacts = true;
    else if (arg.startsWith("--decision=")) out.decision = arg.slice("--decision=".length);
    else if (arg === "--decision") {
      out.decision = argv[i + 1] ?? out.decision;
      i += 1;
    }
  }
  if (out.decision === "rework") out.decision = "rework-then-approve";
  if (!["approve", "cancel", "rework-then-approve"].includes(out.decision)) {
    throw new Error("--decision must be approve, cancel, or rework-then-approve");
  }
  return out;
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      server.close(() => resolve(port));
    });
    server.on("error", reject);
  });
}

async function run(command, args, options = {}) {
  await new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: options.cwd ?? repoRoot,
      stdio: options.stdio ?? "inherit",
      env: options.env ?? process.env,
      shell: false,
    });
    child.on("error", reject);
    child.on("exit", (code) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} ${args.join(" ")} exited with ${code}`));
    });
  });
}

function pipeChildLogs(child, stdoutPath, stderrPath) {
  const stdout = createWriteStream(stdoutPath, { flags: "a" });
  const stderr = createWriteStream(stderrPath, { flags: "a" });
  child.stdout?.on("data", (chunk) => stdout.write(chunk));
  child.stderr?.on("data", (chunk) => stderr.write(chunk));
  child.on("close", () => {
    stdout.end();
    stderr.end();
  });
}

async function readJson(file) {
  return JSON.parse(await readFile(file, "utf8"));
}

async function readJsonl(file) {
  if (!existsSync(file)) return [];
  const text = await readFile(file, "utf8");
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

async function waitFor(label, fn, timeoutMs = 120_000) {
  const started = Date.now();
  let lastError;
  while (Date.now() - started < timeoutMs) {
    try {
      const value = await fn();
      if (value) return value;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`timed out waiting for ${label}${lastError ? `: ${lastError.message}` : ""}`);
}

function apiClient(baseUrl, token) {
  return async function api(method, route, body) {
    const res = await fetch(`${baseUrl}${route}`, {
      method,
      headers: {
        authorization: `Bearer ${token}`,
        "content-type": "application/json",
        "x-tandem-request-source": "control_panel",
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    let parsed = null;
    if (text) {
      try {
        parsed = JSON.parse(text);
      } catch {
        parsed = { raw: text };
      }
    }
    if (!res.ok) {
      throw new Error(`${method} ${route} returned ${res.status}: ${text}`);
    }
    return parsed;
  };
}

function startEventCapture(baseUrl, token, artifactDir) {
  const controller = new AbortController();
  const events = [];
  const eventsPath = path.join(artifactDir, "events.jsonl");
  const ready = (async () => {
    const res = await fetch(`${baseUrl}/event`, {
      headers: { authorization: `Bearer ${token}` },
      signal: controller.signal,
    });
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const frames = buffer.split(/\r?\n\r?\n/);
      buffer = frames.pop() ?? "";
      for (const frame of frames) {
        const data = frame
          .split(/\r?\n/)
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n");
        if (!data) continue;
        try {
          const event = JSON.parse(data);
          events.push(event);
          await writeFile(eventsPath, `${JSON.stringify(event)}\n`, { flag: "a" });
        } catch {
          // Ignore keepalive or truncated frames that are not JSON events.
        }
      }
    }
  })().catch((error) => {
    if (error.name !== "AbortError") {
      console.warn(`event capture stopped: ${error.message}`);
    }
  });
  return { events, stop: () => controller.abort(), ready };
}

function automationSpec(automationId, draft, workspaceDir) {
  return {
    automation_id: automationId,
    name: "Email approval demo",
    description: "Seeded approval-gated email flow using the local email_demo MCP stub.",
    status: "active",
    workspace_root: workspaceDir,
    schedule: {
      type: "manual",
      timezone: "UTC",
      misfire_policy: { type: "run_once" },
    },
    agents: [
      {
        agent_id: "email-demo-agent",
        display_name: "Email Demo Agent",
        model_policy: { default_model: { provider_id: "local", model_id: "echo-1" } },
        tool_policy: { allowlist: [DRAFT_TOOL, SEND_TOOL], denylist: [] },
        mcp_policy: { allowed_servers: ["email_demo"], allowed_tools: ["email.draft", "email.send"] },
      },
    ],
    flow: {
      nodes: [
        {
          node_id: "review_draft_before_send",
          agent_id: "email-demo-agent",
          objective: "Pause for human approval before the seeded email is sent.",
          depends_on: [],
          input_refs: [],
          stage_kind: "approval",
          output_contract: { kind: "approval_gate" },
          gate: {
            required: true,
            decisions: ["approve", "rework", "cancel"],
            rework_targets: [],
            instructions: `Review draft ${draft.draft_id} for ${draft.to} before sending.`,
          },
          metadata: {
            builder: {
              title: "Review draft before send",
              prompt: "Approve only if the seeded draft is safe to send.",
              role: "approver",
            },
            email_demo: { draft },
          },
        },
      ],
    },
    execution: { max_parallel_agents: 1 },
  };
}

function runStatus(body) {
  return body?.run?.status ?? body?.status;
}

async function waitForRun(api, runId, wanted) {
  return waitFor(`run ${runId} to reach ${wanted}`, async () => {
    const body = await api("GET", `/automations/v2/runs/${encodeURIComponent(runId)}`);
    const status = runStatus(body);
    if (status === wanted) return body;
    if (["failed", "blocked"].includes(status)) {
      throw new Error(`run entered ${status}: ${JSON.stringify(body)}`);
    }
    return null;
  });
}

function structuredToolContent(result) {
  return (
    result?.metadata?.result?.structuredContent ??
    result?.metadata?.result?.structured_content ??
    JSON.parse(result.output)
  );
}

async function callEmailTool(api, tool, args) {
  return api("POST", "/tool/execute", {
    tool,
    args,
    scope_allowlist: [DRAFT_TOOL, SEND_TOOL],
  });
}

async function runApprovalRound({ api, workspaceDir, draft, decision, round, artifactDir }) {
  const automationId = `email-approval-demo-${round}-${randomUUID().slice(0, 8)}`;
  await api("POST", "/automations/v2", automationSpec(automationId, draft, workspaceDir));
  const runNow = await api("POST", `/automations/v2/${encodeURIComponent(automationId)}/run_now`, {});
  const runId = runNow.run.run_id;
  const awaiting = await waitForRun(api, runId, "awaiting_approval");
  const approvals = await api("GET", "/approvals/pending?source=automation_v2");
  await writeFile(
    path.join(artifactDir, `approvals-round-${round}.json`),
    `${JSON.stringify(approvals, null, 2)}\n`,
  );

  console.log(`Approval round ${round}: ${runId}`);
  console.log(`Decision endpoint: POST /automations/v2/runs/${runId}/gate`);
  console.log(`Gate title: ${awaiting.run.checkpoint.awaiting_gate?.title ?? "approval gate"}`);

  const gateDecision = await api("POST", `/automations/v2/runs/${encodeURIComponent(runId)}/gate`, {
    decision,
    reason: decision === "approve" ? "Seeded demo approval" : `Seeded demo ${decision}`,
  });

  let finalRun = gateDecision;
  if (decision === "approve") {
    finalRun = await waitForRun(api, runId, "completed");
  } else if (decision === "cancel") {
    finalRun = await waitForRun(api, runId, "cancelled");
  } else {
    await new Promise((resolve) => setTimeout(resolve, 500));
    finalRun = await api("GET", `/automations/v2/runs/${encodeURIComponent(runId)}`);
  }
  return { automationId, runId, decision, awaiting, gateDecision, finalRun };
}

async function chooseDecision(options, rl, round) {
  if (options.nonInteractive) {
    if (options.decision === "rework-then-approve") return round === 1 ? "rework" : "approve";
    return options.decision;
  }
  const answer = (
    await rl.question("Approve, rework, or cancel this email? [approve] ")
  ).trim().toLowerCase();
  if (!answer) return "approve";
  if (["approve", "rework", "cancel"].includes(answer)) return answer;
  console.log("Unknown decision, using cancel.");
  return "cancel";
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const tmpDir = path.join(repoRoot, ".tmp", "email-approval-demo");
  const artifactDir = path.join(tmpDir, "artifacts");
  const workspaceDir = path.join(tmpDir, "workspace");
  const stateDir = path.join(tmpDir, "state");
  const outboxPath = path.join(artifactDir, "outbox.jsonl");
  const draftsPath = path.join(artifactDir, "drafts.jsonl");
  const seedPath = path.join(exampleDir, "seed", "email.json");
  const seed = await readJson(seedPath);

  if (!options.keepArtifacts) {
    await rm(tmpDir, { recursive: true, force: true });
  }
  await mkdir(artifactDir, { recursive: true });
  await mkdir(workspaceDir, { recursive: true });
  await mkdir(stateDir, { recursive: true });
  await copyFile(seedPath, path.join(workspaceDir, "email.json"));

  if (!options.skipBuild) {
    console.log("Building tandem-engine...");
    await run("cargo", ["build", "-p", "tandem-ai", "--bin", "tandem-engine"]);
  }

  const enginePort = await freePort();
  const mcpPort = await freePort();
  const token = `demo-${randomUUID()}`;
  const baseUrl = `http://127.0.0.1:${enginePort}`;
  const mcpUrl = `http://127.0.0.1:${mcpPort}/mcp`;
  const engineBin = path.join(repoRoot, "target", "debug", process.platform === "win32" ? "tandem-engine.exe" : "tandem-engine");
  if (!existsSync(engineBin)) {
    throw new Error(`missing ${engineBin}; run without --skip-build first`);
  }

  const children = [];
  const env = { ...process.env };
  for (const key of PROVIDER_ENV) delete env[key];
  env.TANDEM_STATE_DIR = stateDir;
  env.TANDEM_HOME = stateDir;
  env.TANDEM_MEMORY_DB_PATH = path.join(stateDir, "memory.sqlite");
  env.TANDEM_GLOBAL_CONFIG = path.join(tmpDir, "global-config.json");
  env.TANDEM_DISABLE_EMBEDDINGS = "true";

  const mcp = spawn(process.execPath, [
    path.join(exampleDir, "stub-email-mcp.mjs"),
    "--host",
    "127.0.0.1",
    "--port",
    String(mcpPort),
    "--outbox",
    outboxPath,
    "--drafts",
    draftsPath,
  ], { cwd: repoRoot, env, stdio: ["ignore", "pipe", "pipe"] });
  children.push(mcp);
  pipeChildLogs(mcp, path.join(artifactDir, "mcp.stdout.log"), path.join(artifactDir, "mcp.stderr.log"));

  const engine = spawn(engineBin, [
    "serve",
    "--host",
    "127.0.0.1",
    "--port",
    String(enginePort),
    "--state-dir",
    stateDir,
    "--api-token",
    token,
  ], { cwd: repoRoot, env, stdio: ["ignore", "pipe", "pipe"] });
  children.push(engine);
  pipeChildLogs(engine, path.join(artifactDir, "engine.stdout.log"), path.join(artifactDir, "engine.stderr.log"));

  const cleanup = () => {
    for (const child of children) {
      if (!child.killed) child.kill();
    }
  };
  process.on("SIGINT", () => {
    cleanup();
    process.exit(130);
  });
  process.on("SIGTERM", () => {
    cleanup();
    process.exit(143);
  });

  try {
    await waitFor("MCP stub", async () => {
      const res = await fetch(mcpUrl, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: "probe", method: "initialize", params: {} }),
      });
      return res.ok;
    }, 15_000);
    await waitFor("engine health", async () => {
      const res = await fetch(`${baseUrl}/global/health`, {
        headers: { authorization: `Bearer ${token}` },
      });
      if (!res.ok) return false;
      const body = await res.json();
      return body.ready === true || body.phase === "ready";
    }, 120_000);

    const events = startEventCapture(baseUrl, token, artifactDir);
    const api = apiClient(baseUrl, token);
    const rl = options.nonInteractive ? null : createInterface({ input, output });

    await api("POST", "/mcp", {
      name: "email_demo",
      transport: mcpUrl,
      allowed_tools: ["email.draft", "email.send"],
      purpose: "demo_email",
      grounding_required: false,
    });
    await api("POST", "/mcp/email_demo/connect");
    await waitFor("email_demo bridge tools", async () => {
      const ids = await api("GET", "/tool/ids");
      return Array.isArray(ids) && ids.includes(DRAFT_TOOL) && ids.includes(SEND_TOOL);
    });

    const draftResult1 = await callEmailTool(api, DRAFT_TOOL, {
      to: seed.to,
      subject: seed.subject,
      body: seed.body,
    });
    let draft = structuredToolContent(draftResult1);
    const rounds = [];
    const firstDecision = await chooseDecision(options, rl, 1);
    rounds.push(await runApprovalRound({ api, workspaceDir, draft, decision: firstDecision, round: 1, artifactDir }));

    if (firstDecision === "rework") {
      const draftResult2 = await callEmailTool(api, DRAFT_TOOL, {
        to: seed.to,
        subject: `${seed.subject} (revised)`,
        body: seed.rework_body,
      });
      draft = structuredToolContent(draftResult2);
      const secondDecision = await chooseDecision(options, rl, 2);
      rounds.push(await runApprovalRound({ api, workspaceDir, draft, decision: secondDecision, round: 2, artifactDir }));
    }

    const finalRound = rounds.at(-1);
    let sendResult = null;
    if (finalRound.decision === "approve") {
      sendResult = await callEmailTool(api, SEND_TOOL, {
        draft_id: draft.draft_id,
        to: draft.to,
        subject: draft.subject,
        body: draft.body,
        approved_by: "demo-reviewer",
        run_id: finalRound.runId,
      });
    }

    if (rl) rl.close();
    await new Promise((resolve) => setTimeout(resolve, 750));
    events.stop();
    await events.ready;

    const outbox = await readJsonl(outboxPath);
    const drafts = await readJsonl(draftsPath);
    const toolDispatchEvents = events.events.filter((event) => event.type === "tool.dispatch.recorded");
    const gateEvents = events.events.filter((event) => event.type === "approval.decision.recorded");
    const gateHistory = rounds.flatMap((round) => round.finalRun?.run?.checkpoint?.gate_history ?? []);
    const evidence = {
      ok: finalRound.decision === "approve" ? outbox.length > 0 : outbox.length === 0,
      base_url: baseUrl,
      mcp_url: mcpUrl,
      artifact_dir: artifactDir,
      rounds: rounds.map((round) => ({
        automation_id: round.automationId,
        run_id: round.runId,
        decision: round.decision,
        final_status: runStatus(round.finalRun),
      })),
      gate_history: gateHistory,
      approval_events: gateEvents,
      tool_dispatch_events: toolDispatchEvents,
      drafts,
      outbox,
      send_result: sendResult,
    };
    await writeFile(path.join(artifactDir, "evidence.json"), `${JSON.stringify(evidence, null, 2)}\n`);

    if (finalRound.decision === "approve" && outbox.length === 0) {
      throw new Error("approval path completed without an outbox send");
    }
    if (gateHistory.length === 0) {
      throw new Error("approval flow completed without gate_history evidence");
    }
    if (!toolDispatchEvents.some((event) => event.properties?.canonical_tool === SEND_TOOL)) {
      if (finalRound.decision === "approve") {
        throw new Error("approval path completed without email.send tool dispatch evidence");
      }
    }

    console.log("");
    console.log("Email approval demo PASS");
    console.log(`Artifacts: ${artifactDir}`);
    console.log(`Gate decisions: ${gateHistory.map((record) => record.decision).join(", ")}`);
    console.log(`Tool dispatch events: ${toolDispatchEvents.length}`);
    console.log(`Outbox messages: ${outbox.length}`);
  } finally {
    cleanup();
  }
}

main().catch((error) => {
  console.error(`Email approval demo FAILED: ${error.stack ?? error.message}`);
  process.exitCode = 1;
});
