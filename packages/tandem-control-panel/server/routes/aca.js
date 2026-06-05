import { existsSync } from "fs";
import { mkdir, readFile, rename, rm, writeFile } from "fs/promises";
import { basename, dirname, join, resolve } from "path";
import { randomBytes } from "crypto";

function copyRequestHeaders(req) {
  const headers = new Headers();
  for (const [key, value] of Object.entries(req.headers || {})) {
    if (!value) continue;
    const lower = key.toLowerCase();
    if (["host", "content-length", "cookie", "authorization"].includes(lower)) {
      continue;
    }
    if (Array.isArray(value)) headers.set(key, value.join(", "));
    else headers.set(key, value);
  }
  return headers;
}

async function readJsonBody(req) {
  let raw = "";
  for await (const chunk of req) raw += chunk;
  if (!raw.trim()) return {};
  return JSON.parse(raw);
}

function safeSegment(value, fallback) {
  const text = String(value || "").trim();
  return (text || fallback).replace(/[^A-Za-z0-9_.-]+/g, "_").slice(0, 120);
}

async function writeTextFileAtomic(pathname, content) {
  const target = resolve(String(pathname || "").trim());
  await mkdir(dirname(target), { recursive: true });
  const temp = join(
    dirname(target),
    `${basename(target)}.${process.pid}.${Date.now()}.${randomBytes(4).toString("hex")}.tmp`
  );
  await writeFile(temp, content, "utf8");
  try {
    await rename(temp, target);
  } catch (error) {
    await rm(temp, { force: true }).catch(() => {});
    throw error;
  }
}

function normalizeSourceRefs(value) {
  if (!value || typeof value !== "object") return {};
  const out = {};
  for (const [key, raw] of Object.entries(value)) {
    const text = String(raw ?? "").trim();
    if (text) out[key] = text;
  }
  return out;
}

function createFeedbackStore(options = {}) {
  const stateDir = resolve(
    String(options.stateDir || "").trim() || resolve(process.cwd(), "tandem-data", "control-panel")
  );
  const root = resolve(stateDir, "aca-feedback");
  const subscribers = new Map();
  const runLocks = new Map();

  function fileForRun(runId) {
    return join(root, `${safeSegment(runId, "run")}.json`);
  }

  async function readRun(runId) {
    const file = fileForRun(runId);
    if (!existsSync(file)) return { next_seq: 1, messages: [] };
    const parsed = JSON.parse(await readFile(file, "utf8"));
    const messages = Array.isArray(parsed?.messages) ? parsed.messages : [];
    const maxSeq = messages.reduce((max, message) => Math.max(max, Number(message?.seq || 0)), 0);
    return {
      next_seq: Math.max(Number(parsed?.next_seq || 0), maxSeq + 1, 1),
      messages,
    };
  }

  async function writeRun(runId, record) {
    await writeTextFileAtomic(fileForRun(runId), `${JSON.stringify(record, null, 2)}\n`);
  }

  function publish(runId, event) {
    const clients = subscribers.get(runId);
    if (!clients) return;
    const line = `data: ${JSON.stringify(event)}\n\n`;
    for (const res of [...clients]) {
      try {
        res.write(line);
      } catch {
        clients.delete(res);
      }
    }
  }

  function subscribe(runId, res) {
    const key = String(runId || "").trim();
    if (!subscribers.has(key)) subscribers.set(key, new Set());
    subscribers.get(key).add(res);
    return () => {
      const clients = subscribers.get(key);
      if (!clients) return;
      clients.delete(res);
      if (!clients.size) subscribers.delete(key);
    };
  }

  async function withRunLock(runId, fn) {
    const key = String(runId || "").trim();
    const previous = runLocks.get(key) || Promise.resolve();
    let release;
    const next = new Promise((resolveRelease) => {
      release = resolveRelease;
    });
    const chained = previous.then(() => next, () => next);
    runLocks.set(key, chained);
    await previous.catch(() => {});
    try {
      return await fn();
    } finally {
      release();
      if (runLocks.get(key) === chained) runLocks.delete(key);
    }
  }

  return {
    async list(runId, sinceSeq = 0) {
      const record = await readRun(runId);
      return record.messages.filter((message) => Number(message?.seq || 0) > sinceSeq);
    },
    async add(runId, input) {
      return withRunLock(runId, async () => {
        const record = await readRun(runId);
        const seq = record.next_seq;
        const now = Date.now();
        const message = {
          id: `${safeSegment(runId, "run")}-${seq}`,
          seq,
          run_id: runId,
          task_id: String(input?.task_id || input?.taskId || "").trim(),
          thread_id: String(input?.thread_id || input?.threadId || runId).trim(),
          actor: String(input?.actor || input?.author || "operator").trim(),
          kind: String(input?.kind || "operator_feedback").trim(),
          body: String(input?.body || input?.message || input?.text || "").trim(),
          created_at_ms: now,
          updated_at_ms: now,
          delivery_state: "pending",
          delivered_at_ms: null,
          source_refs: normalizeSourceRefs(input?.source_refs || input?.sourceRefs),
        };
        if (!message.body) throw new Error("Missing feedback body.");
        record.messages.push(message);
        record.next_seq = seq + 1;
        await writeRun(runId, record);
        publish(runId, {
          event_type: "operator_feedback",
          seq,
          run_id: runId,
          timestamp_ms: now,
          payload: { message },
        });
        return message;
      });
    },
    async updateDelivery(runId, messageId, patch) {
      return withRunLock(runId, async () => {
        const record = await readRun(runId);
        const index = record.messages.findIndex((message) => message?.id === messageId);
        if (index < 0) return null;
        record.messages[index] = { ...record.messages[index], ...patch, updated_at_ms: Date.now() };
        await writeRun(runId, record);
        publish(runId, {
          event_type: "operator_feedback_delivery",
          seq: Number(record.messages[index].seq || 0),
          run_id: runId,
          timestamp_ms: Date.now(),
          payload: { message: record.messages[index] },
        });
        return record.messages[index];
      });
    },
    async pending(runId) {
      const record = await readRun(runId);
      return record.messages.filter((message) => message?.delivery_state !== "delivered");
    },
    subscribe,
  };
}

export function createAcaApiHandler(deps) {
  const { PORTAL_PORT, ACA_BASE_URL, getAcaToken, sendJson } = deps;
  const feedbackStore = createFeedbackStore({ stateDir: deps.TANDEM_CONTROL_PANEL_STATE_DIR });

  async function deliverFeedback(baseUrl, message) {
    if (!baseUrl) return { ok: false, state: "pending", error: "aca_not_configured" };
    const token = String(getAcaToken?.() || "aca-proxy").trim();
    const headers = new Headers({ "content-type": "application/json", accept: "application/json" });
    if (token) headers.set("authorization", `Bearer ${token}`);
    try {
      const res = await fetch(`${baseUrl}/operator/feedback`, {
        method: "POST",
        headers,
        body: JSON.stringify({ message }),
      });
      if (!res.ok) return { ok: false, state: "pending", error: `aca_feedback_${res.status}` };
      return { ok: true, state: "delivered", error: "" };
    } catch (error) {
      return {
        ok: false,
        state: "pending",
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }

  async function replayPendingFeedback(baseUrl, runId) {
    const pending = await feedbackStore.pending(runId);
    const delivered = [];
    for (const message of pending.sort((a, b) => Number(a.seq || 0) - Number(b.seq || 0))) {
      const delivery = await deliverFeedback(baseUrl, message);
      const next = await feedbackStore.updateDelivery(runId, message.id, {
        delivery_state: delivery.state,
        delivered_at_ms: delivery.ok ? Date.now() : null,
        delivery_error: delivery.error || "",
      });
      delivered.push(next || message);
      if (!delivery.ok) break;
    }
    return delivered;
  }

  return async function handleAcaApi(req, res) {
    const baseUrl = String(ACA_BASE_URL || "").trim().replace(/\/+$/, "");

    const incoming = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`);
    const targetPath = incoming.pathname.replace(/^\/api\/aca/, "") || "/";
    const feedbackMatch = targetPath.match(/^\/runs\/([^/]+)\/feedback(?:\/(events|replay))?$/);
    if (feedbackMatch) {
      const runId = decodeURIComponent(feedbackMatch[1] || "").trim();
      const mode = feedbackMatch[2] || "";
      if (!runId) {
        sendJson(res, 400, { ok: false, error: "Missing run id." });
        return true;
      }
      if (!mode && req.method === "GET") {
        const sinceSeq = Number(incoming.searchParams.get("since_seq") || 0);
        const messages = await feedbackStore.list(runId, Number.isFinite(sinceSeq) ? sinceSeq : 0);
        sendJson(res, 200, { ok: true, run_id: runId, messages });
        return true;
      }
      if (!mode && req.method === "POST") {
        const body = await readJsonBody(req);
        const message = await feedbackStore.add(runId, body);
        const delivery = await deliverFeedback(baseUrl, message);
        const delivered = await feedbackStore.updateDelivery(runId, message.id, {
          delivery_state: delivery.state,
          delivered_at_ms: delivery.ok ? Date.now() : null,
          delivery_error: delivery.error || "",
        });
        sendJson(res, 201, { ok: true, run_id: runId, message: delivered || message });
        return true;
      }
      if (mode === "replay" && req.method === "POST") {
        const messages = await replayPendingFeedback(baseUrl, runId);
        sendJson(res, 200, { ok: true, run_id: runId, messages });
        return true;
      }
      if (mode === "events" && req.method === "GET") {
        const sinceSeq = Number(incoming.searchParams.get("since_seq") || 0);
        res.writeHead(200, {
          "content-type": "text/event-stream",
          "cache-control": "no-cache",
          connection: "keep-alive",
        });
        res.write(
          `data: ${JSON.stringify({
            event_type: "hello",
            run_id: runId,
            timestamp_ms: Date.now(),
          })}\n\n`
        );
        const replay = await feedbackStore.list(runId, Number.isFinite(sinceSeq) ? sinceSeq : 0);
        for (const message of replay) {
          res.write(
            `data: ${JSON.stringify({
              event_type: "operator_feedback",
              seq: Number(message.seq || 0),
              run_id: runId,
              timestamp_ms: Number(message.created_at_ms || Date.now()),
              payload: { message },
            })}\n\n`
          );
        }
        const unsubscribe = feedbackStore.subscribe(runId, res);
        req.on("close", unsubscribe);
        return true;
      }
      sendJson(res, 405, { ok: false, error: "Unsupported feedback route." });
      return true;
    }

    if (!baseUrl) {
      sendJson(res, 503, {
        ok: false,
        error: "ACA integration is not configured. Set ACA_BASE_URL to enable ACA-backed coding.",
      });
      return true;
    }

    if (targetPath === "/overview" && req.method === "GET") {
      const token = String(getAcaToken?.() || "aca-proxy").trim();
      const headers = copyRequestHeaders(req);
      if (token) headers.set("authorization", `Bearer ${token}`);
      headers.set("content-type", "application/json");
      headers.set("accept", "application/json");

      let upstream;
      try {
        upstream = await fetch(`${baseUrl}/mcp`, {
          method: "POST",
          headers,
          body: JSON.stringify({
            jsonrpc: "2.0",
            id: "aca-overview",
            method: "tools/call",
            params: { name: "describe_aca", arguments: {} },
          }),
        });
      } catch (error) {
        sendJson(res, 502, {
          ok: false,
          error: `ACA overview is unavailable: ${error instanceof Error ? error.message : String(error)}`,
        });
        return true;
      }

      let payload;
      try {
        payload = await upstream.json();
      } catch {
        payload = null;
      }

      if (!upstream.ok) {
        sendJson(res, upstream.status, {
          ok: false,
          error: String(payload?.error?.message || payload?.detail || `ACA overview failed (${upstream.status})`),
        });
        return true;
      }

      const overview = payload?.result?.overview;
      if (!overview || typeof overview !== "object") {
        sendJson(res, 502, {
          ok: false,
          error: "ACA overview tool returned an unexpected payload.",
        });
        return true;
      }

      sendJson(res, 200, {
        ok: true,
        source: "aca-mcp",
        fetched_at_ms: Date.now(),
        overview,
      });
      return true;
    }

    const targetUrl = `${baseUrl}${targetPath}${incoming.search}`;
    const token = String(getAcaToken?.() || "aca-proxy").trim();
    const needsAuth = targetPath !== "/health";

    const headers = copyRequestHeaders(req);
    if (needsAuth && token) headers.set("authorization", `Bearer ${token}`);
    if (!headers.has("accept")) headers.set("accept", "*/*");

    const hasBody = !["GET", "HEAD"].includes(req.method || "GET");

    let upstream;
    try {
      upstream = await fetch(targetUrl, {
        method: req.method,
        headers,
        body: hasBody ? req : undefined,
        duplex: hasBody ? "half" : undefined,
      });
    } catch (error) {
      sendJson(res, 502, {
        ok: false,
        error: `ACA unreachable: ${error instanceof Error ? error.message : String(error)}`,
      });
      return true;
    }

    const responseHeaders = {};
    upstream.headers.forEach((value, key) => {
      const lower = key.toLowerCase();
      if (["content-encoding", "transfer-encoding", "connection"].includes(lower)) return;
      responseHeaders[key] = value;
    });

    try {
      res.writeHead(upstream.status, responseHeaders);
      if (!upstream.body) {
        res.end();
        return true;
      }
      for await (const chunk of upstream.body) {
        if (res.writableEnded || res.destroyed) break;
        res.write(chunk);
      }
      if (!res.writableEnded && !res.destroyed) {
        res.end();
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (res.headersSent) {
        const lower = message.toLowerCase();
        if (lower.includes("terminated") || lower.includes("aborted")) {
          if (!res.writableEnded && !res.destroyed) res.end();
          return true;
        }
        if (!res.destroyed && !res.writableEnded) {
          res.destroy(error instanceof Error ? error : undefined);
        }
        return true;
      }
      sendJson(res, 502, {
        ok: false,
        error: `ACA proxy stream failed: ${message}`,
      });
    }

    return true;
  };
}
