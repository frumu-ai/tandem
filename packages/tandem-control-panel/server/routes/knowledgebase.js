import { readFileSync } from "node:fs";

function readOptionalTokenFile(pathname) {
  const target = String(pathname || "").trim();
  if (!target) return "";
  try {
    return readFileSync(target, "utf8").trim();
  } catch {
    return "";
  }
}

function normalizeBaseUrl(raw) {
  return String(raw || "").trim().replace(/\/+$/, "");
}

function copyForwardHeaders(req) {
  const headers = new Headers();
  for (const [key, value] of Object.entries(req.headers || {})) {
    if (value == null || value === "") continue;
    const lower = key.toLowerCase();
    if (["host", "cookie", "authorization", "content-length", "connection", "transfer-encoding"].includes(lower)) {
      continue;
    }
    if (Array.isArray(value)) headers.set(key, value.join(", "));
    else headers.set(key, String(value));
  }
  return headers;
}

async function proxyResponse(res, upstream) {
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
      return;
    }
    for await (const chunk of upstream.body) {
      if (res.writableEnded || res.destroyed) break;
      res.write(chunk);
    }
    if (!res.writableEnded && !res.destroyed) res.end();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (res.headersSent) {
      if (!res.writableEnded && !res.destroyed) res.end();
      return;
    }
    res.writeHead(502, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: false, error: `KB proxy stream failed: ${message}` }));
  }
}

function readKbAdminToken(deps) {
  return (
    String(deps.TANDEM_KB_ADMIN_API_KEY || deps.KB_ADMIN_API_KEY || "").trim() ||
    readOptionalTokenFile(deps.TANDEM_KB_ADMIN_API_KEY_FILE || deps.KB_ADMIN_API_KEY_FILE || "")
  );
}

async function probeKbAdminConfig(baseUrl, deps) {
  const headers = new Headers();
  headers.set("accept", "application/json");
  const token = readKbAdminToken(deps);
  if (token) headers.set("authorization", `Bearer ${token}`);
  const timeoutMs = Number(deps.TANDEM_KB_ADMIN_PROBE_TIMEOUT_MS || deps.KB_ADMIN_PROBE_TIMEOUT_MS || 2000);
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), Number.isFinite(timeoutMs) ? timeoutMs : 2000);
  try {
    const upstream = await fetch(`${baseUrl}/admin/collections`, {
      method: "GET",
      headers,
      signal: controller.signal,
    });
    const text = await upstream.text().catch(() => "");
    let payload = {};
    if (text) {
      try {
        payload = JSON.parse(text);
      } catch {
        payload = {};
      }
    }
    if (!upstream.ok) {
      return {
        reachable: false,
        status: upstream.status,
        error: payload?.error || text || `KB admin collections returned ${upstream.status}`,
      };
    }
    return {
      reachable: true,
      status: upstream.status,
    };
  } catch (error) {
    return {
      reachable: false,
      status: 0,
      error: error instanceof Error ? error.message : String(error),
    };
  } finally {
    clearTimeout(timer);
  }
}

function encodeIncomingPathSegment(segment) {
  const raw = String(segment || "");
  try {
    return encodeURIComponent(decodeURIComponent(raw));
  } catch {
    return encodeURIComponent(raw);
  }
}

function resolveTargetPath(pathname) {
  const parts = String(pathname || "").split("/").filter(Boolean);
  if (parts[0] !== "api" || parts[1] !== "knowledgebase") return "";
  const rest = parts.slice(2);
  if (!rest.length) return "";

  if (rest[0] === "collections") return "/admin/collections";
  if (rest[0] === "documents" && rest.length === 1) return "/admin/documents";
  if (rest[0] === "documents" && rest.length >= 3) {
    return `/admin/documents/${encodeIncomingPathSegment(rest[1])}/${encodeIncomingPathSegment(rest[2])}`;
  }
  if (rest[0] === "reindex") return "/admin/reindex";
  if (rest[0] === "config") return "/admin/config";
  return "";
}

export function createKnowledgebaseApiHandler(deps) {
  const {
    PORTAL_PORT,
    TANDEM_KB_ADMIN_URL,
    sendJson,
  } = deps;

  return async function handleKnowledgebaseApi(req, res) {
    const incoming = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`);
    const targetPath = resolveTargetPath(incoming.pathname);
    if (!targetPath) {
      sendJson(res, 404, { ok: false, error: "Unknown knowledgebase route." });
      return true;
    }

    const baseUrl = normalizeBaseUrl(TANDEM_KB_ADMIN_URL || deps.KB_ADMIN_URL || "");
    if (!baseUrl) {
      sendJson(res, 503, {
        ok: false,
        error: "Knowledgebase admin URL is not configured. Set TANDEM_KB_ADMIN_URL to enable KB uploads.",
      });
      return true;
    }

    if (incoming.pathname === "/api/knowledgebase/config" && req.method === "GET") {
      const probe = await probeKbAdminConfig(baseUrl, deps);
      sendJson(res, 200, {
        ok: true,
        configured: probe.reachable,
        reachable: probe.reachable,
        status: probe.status,
        error: probe.reachable ? undefined : probe.error,
        admin_url: baseUrl,
        default_collection_id: String(deps.TANDEM_KB_DEFAULT_COLLECTION_ID || deps.KB_DEFAULT_COLLECTION_ID || "").trim(),
      });
      return true;
    }

    const method = req.method || "GET";
    if (
      !["GET", "POST", "PUT", "DELETE", "PATCH"].includes(method) ||
      (incoming.pathname === "/api/knowledgebase/collections" && method !== "GET") ||
      (incoming.pathname === "/api/knowledgebase/documents" && !["GET", "POST"].includes(method)) ||
      (incoming.pathname.startsWith("/api/knowledgebase/documents/") && !["GET", "PUT", "DELETE"].includes(method)) ||
      (incoming.pathname === "/api/knowledgebase/reindex" && method !== "POST")
    ) {
      sendJson(res, 405, { ok: false, error: "Method not allowed." });
      return true;
    }

    const targetUrl = `${baseUrl}${targetPath}${incoming.search}`;
    const headers = copyForwardHeaders(req);
    const token = readKbAdminToken(deps);
    if (token) headers.set("authorization", `Bearer ${token}`);
    if (!headers.has("accept")) headers.set("accept", "application/json");

    const hasBody = !["GET", "HEAD"].includes(method);

    let upstream;
    try {
      upstream = await fetch(targetUrl, {
        method,
        headers,
        body: hasBody ? req : undefined,
        duplex: hasBody ? "half" : undefined,
      });
    } catch (error) {
      sendJson(res, 502, {
        ok: false,
        error: `Knowledgebase admin unreachable: ${error instanceof Error ? error.message : String(error)}`,
      });
      return true;
    }

    await proxyResponse(res, upstream);
    return true;
  };
}
