import { setTimeout as sleep } from "timers/promises";

const BASE_URL = process.env.TANDEM_ENGINE_URL || "http://127.0.0.1:39731";
const TOKEN = process.env.TANDEM_TOKEN || "";
const RUNS = Number(process.argv[2] || "50");
const CONCURRENCY = Number(process.argv[3] || "10");
const PROMPT = process.argv[4] || "Write a single sentence about autonomous coding.";

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

async function createSession() {
  const payload = { title: "LoadTest", directory: "." };
  const data = await request("/session", { method: "POST", body: JSON.stringify(payload) });
  return data.id;
}

async function promptAsync(sessionId) {
  const payload = { parts: [{ type: "text", text: PROMPT }] };
  const url = `/session/${encodeURIComponent(sessionId)}/prompt_async?return=run`;
  const res = await fetch(`${BASE_URL}${url}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${TOKEN}`,
    },
    body: JSON.stringify(payload),
  });
  if (res.status === 409) {
    const conflict = await res.json().catch(() => ({}));
    const active = conflict.activeRun || {};
    const id =
      active.runID || active.runId || active.run_id || (conflict.run && conflict.run.id) || null;
    if (!id) throw new Error("conflict without run id");
    return id;
  }
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`promptAsync failed ${res.status} ${body}`);
  }
  const data = await res.json().catch(() => ({}));
  const id =
    (data.run && (data.run.id || data.runId || data.runID)) ||
    data.id ||
    data.runId ||
    data.runID ||
    null;
  if (!id) throw new Error("missing run id");
  return id;
}

async function streamUntilDone(sessionId, runId, signal) {
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

async function runOnce() {
  const start = Date.now();
  const controller = new AbortController();
  let sessionId = "";
  try {
    sessionId = await createSession();
    const runId = await promptAsync(sessionId);
    const doneType = await streamUntilDone(sessionId, runId, controller.signal);
    const elapsedMs = Date.now() - start;
    return { ok: true, elapsedMs, doneType };
  } catch (err) {
    const elapsedMs = Date.now() - start;
    return { ok: false, elapsedMs, error: String(err && err.message ? err.message : err) };
  } finally {
    try {
      if (sessionId) {
        await request(`/session/${encodeURIComponent(sessionId)}`, { method: "DELETE" });
      }
    } catch {}
    controller.abort();
  }
}

async function main() {
  const results = [];
  const queue = Array.from({ length: RUNS }, (_, i) => i);
  const active = new Set();
  let launched = 0;
  while (results.length < RUNS) {
    while (active.size < CONCURRENCY && queue.length) {
      const idx = queue.shift();
      launched++;
      const p = runOnce().then((r) => {
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
  const avgMs = ok.length ? Math.round(ok.reduce((a, r) => a + r.elapsedMs, 0) / ok.length) : 0;
  const p95Ms = ok.length
    ? ok
        .map((r) => r.elapsedMs)
        .sort((a, b) => a - b)[Math.floor(ok.length * 0.95) - 1] || 0
    : 0;
  const doneTypes = {};
  for (const r of ok) {
    const t = r.doneType || "unknown";
    doneTypes[t] = (doneTypes[t] || 0) + 1;
  }
  const summary = {
    baseUrl: BASE_URL,
    runsRequested: RUNS,
    concurrency: CONCURRENCY,
    launched,
    ok: ok.length,
    failed: fail.length,
    averageMs: avgMs,
    p95Ms,
    doneTypes,
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
