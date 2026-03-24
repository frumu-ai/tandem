import { setTimeout as sleep } from "timers/promises";

const BASE_URL = process.env.TANDEM_ENGINE_URL || "http://127.0.0.1:39731";
const TOKEN = process.env.TANDEM_TOKEN || "";
const MISSIONS = Number(process.argv[2] || "10");
const WORKERS = Number(process.argv[3] || "3");
const CONCURRENCY = Number(process.argv[4] || "5");
const TEMPLATE_ID = process.argv[5] || "worker-default";
const PROMPT = process.argv[6] || "Summarize the repository structure in one sentence.";

async function request(path, init = {}) {
  const res = await fetch(`${BASE_URL}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${TOKEN}`,
      ...(init.headers || {}),
    },
  });
  const text = await res.text().catch(() => "");
  if (!res.ok) {
    let json;
    try {
      json = JSON.parse(text);
    } catch {}
    throw new Error(`HTTP ${res.status} ${res.statusText} ${json ? JSON.stringify(json) : text}`);
  }
  try {
    return JSON.parse(text);
  } catch {
    return {};
  }
}

async function ensureWorkerTemplate() {
  const list = await request("/agent-team/templates");
  const templates = Array.isArray(list.templates) ? list.templates : [];
  const found = templates.find((t) => t.templateID === TEMPLATE_ID);
  if (found) return found.templateID;
  const payload = {
    template: {
      templateID: TEMPLATE_ID,
      display_name: "Worker",
      role: "worker",
      system_prompt: "You are a worker agent that performs implementation tasks concisely.",
      default_model: null,
      skills: [],
      default_budget: {},
      capabilities: {
        tool_allowlist: ["read", "glob", "grep", "apply_patch", "edit"],
        tool_denylist: [],
        fs_scopes: { read: ["**/*"], write: ["**/*"] },
        net_scopes: { enabled: false, allow_hosts: [] },
        secrets_scopes: [],
        git_caps: { read: true, commit: true, push: false, push_requires_approval: true },
      },
    },
  };
  const created = await request("/agent-team/templates", {
    method: "POST",
    body: JSON.stringify(payload),
  });
  const id =
    created.template?.templateID ||
    created.templateID ||
    created.template?.template_id ||
    TEMPLATE_ID;
  return id;
}

async function createMission(title, goal, workers) {
  const work_items = Array.from({ length: workers }, (_, i) => ({
    work_item_id: null,
    title: `Work Item ${i + 1}`,
    detail: PROMPT,
    assigned_agent: "worker",
  }));
  const payload = { title, goal, work_items };
  const res = await request("/mission", { method: "POST", body: JSON.stringify(payload) });
  const mission = res.mission || {};
  return mission.mission_id || mission.missionID || mission.id;
}

async function startMission(missionId) {
  const payload = { event: { type: "mission_started", mission_id: missionId } };
  const res = await request(`/mission/${encodeURIComponent(missionId)}/event`, {
    method: "POST",
    body: JSON.stringify(payload),
  });
  const spawns = Array.isArray(res.orchestratorSpawns) ? res.orchestratorSpawns : [];
  return spawns.map((row) => ({
    ok: !!row.ok,
    instanceID: row.instanceID || null,
    sessionID: row.sessionID || null,
    status: row.status || null,
    workItemID: row.workItemID || null,
    code: row.code || null,
    error: row.error || null,
  }));
}

async function listInstancesByMission(missionId) {
  const params = new URLSearchParams({ missionID: missionId });
  const res = await request(`/agent-team/instances?${params.toString()}`);
  const instances = Array.isArray(res.instances) ? res.instances : [];
  return instances.map((x) => ({
    instanceID: x.instanceID || x.instance_id,
    missionID: x.missionID || x.mission_id,
    sessionID: x.sessionID || x.session_id,
    runID: x.runID || x.run_id || null,
    status: x.status || null,
    role: x.role || null,
    templateID: x.templateID || x.template_id,
  }));
}

async function streamUntilDone(sessionId, runId, signal) {
  if (!runId) return "no_run_id";
  const params = new URLSearchParams({ sessionID: sessionId, runID: runId });
  const res = await fetch(`${BASE_URL}/event?${params.toString()}`, {
    headers: { authorization: `Bearer ${TOKEN}` },
    signal,
  });
  if (!res.ok || !res.body) throw new Error(`stream failed ${res.status}`);
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  const doneTypes = new Set(["run.complete", "run.completed", "run.failed", "session.run.finished"]);
  for (;;) {
    const chunk = await reader.read();
    if (chunk.done) break;
    buffer += decoder.decode(chunk.value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() || "";
    for (const line of lines) {
      if (line.startsWith("data:")) {
        const json = line.slice(5).trim();
        let evt;
        try {
          evt = JSON.parse(json);
        } catch {
          continue;
        }
        const t = evt.type || "";
        if (doneTypes.has(String(t))) return t;
      }
    }
  }
  return "stream.end";
}

async function runMission(index, workers) {
  const controller = new AbortController();
  const startMs = Date.now();
  try {
    const missionId = await createMission(
      `LoadTest Mission ${index + 1}`,
      "Execute worker tasks concurrently.",
      workers
    );
    const spawns = await startMission(missionId);
    const okSpawns = spawns.filter((s) => s.ok);
    const denied = spawns.filter((s) => !s.ok);
    const instances = await listInstancesByMission(missionId);
    const workerRuns = instances.filter((x) => x.role === "worker" || String(x.role).toLowerCase() === "worker");
    const completions = [];
    for (const w of workerRuns) {
      try {
        const t = await streamUntilDone(w.sessionID, w.runID, controller.signal);
        completions.push({ instanceID: w.instanceID, runID: w.runID, type: t, ok: t.includes("completed") });
      } catch (err) {
        completions.push({ instanceID: w.instanceID, runID: w.runID, type: "error", ok: false, error: String(err?.message || err) });
      }
    }
    const elapsedMs = Date.now() - startMs;
    return {
      ok: true,
      missionId,
      elapsedMs,
      spawnOk: okSpawns.length,
      spawnDenied: denied.length,
      deniedCodes: denied.reduce((acc, s) => {
        const key = s.code || "unknown";
        acc[key] = (acc[key] || 0) + 1;
        return acc;
      }, {}),
      completions,
    };
  } catch (err) {
    return { ok: false, error: String(err?.message || err) };
  } finally {
    controller.abort();
  }
}

async function main() {
  const workerTemplateId = await ensureWorkerTemplate();
  const results = [];
  const queue = Array.from({ length: MISSIONS }, (_, i) => i);
  const active = new Set();
  while (results.length < MISSIONS) {
    while (active.size < CONCURRENCY && queue.length) {
      const i = queue.shift();
      const p = runMission(i, WORKERS).then((r) => {
        active.delete(p);
        results.push(r);
      });
      active.add(p);
    }
    if (active.size) await Promise.race(active);
    else await sleep(10);
  }
  const ok = results.filter((r) => r.ok);
  const fail = results.filter((r) => !r.ok);
  const avgMs = ok.length ? Math.round(ok.reduce((a, r) => a + (r.elapsedMs || 0), 0) / ok.length) : 0;
  const p95Ms = ok.length
    ? ok
        .map((r) => r.elapsedMs || 0)
        .sort((a, b) => a - b)[Math.max(0, Math.floor(ok.length * 0.95) - 1)]
    : 0;
  const spawnDenied = ok.reduce((a, r) => a + (r.spawnDenied || 0), 0);
  const deniedCodes = ok.reduce((acc, r) => {
    const m = r.deniedCodes || {};
    Object.keys(m).forEach((k) => (acc[k] = (acc[k] || 0) + m[k]));
    return acc;
  }, {});
  const completionTypes = ok
    .flatMap((r) => r.completions || [])
    .reduce((acc, c) => {
      const key = c.type || "unknown";
      acc[key] = (acc[key] || 0) + 1;
      return acc;
    }, {});
  const summary = {
    baseUrl: BASE_URL,
    missionsRequested: MISSIONS,
    workersPerMission: WORKERS,
    concurrency: CONCURRENCY,
    templateID: workerTemplateId,
    okMissions: ok.length,
    failedMissions: fail.length,
    averageMs: avgMs,
    p95Ms,
    spawnDeniedTotal: spawnDenied,
    deniedCodes,
    completionTypes,
    errors: Object.entries(
      fail.reduce((acc, r) => {
        const key = r.error || "unknown";
        acc[key] = (acc[key] || 0) + 1;
        return acc;
      }, {})
    )
      .sort((a, b) => b[1] - a[1])
      .slice(0, 10),
  };
  process.stdout.write(JSON.stringify(summary, null, 2) + "\n");
}

main().catch((err) => {
  process.stderr.write(String(err && err.stack ? err.stack : err) + "\n");
  process.exit(1);
});
