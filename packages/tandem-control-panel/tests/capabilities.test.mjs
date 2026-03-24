import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import {
  createCapabilitiesHandler,
  getCapabilitiesMetrics,
  resetCapabilitiesCache,
  resetCapabilitiesState,
} from "../server/routes/capabilities.js";

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

test.beforeEach(() => {
  resetCapabilitiesState();
});

test.afterEach(() => {
  resetCapabilitiesCache();
  resetCapabilitiesState();
});

test("ACA not configured returns aca_not_configured and does not crash", () => {
  const metrics = getCapabilitiesMetrics();
  assert.equal(metrics.aca_probe_error_counts.aca_not_configured >= 0, true);

  let sentStatus = 0;
  let sentBody = null;
  const handler = createCapabilitiesHandler({
    ACA_BASE_URL: "",
    engineHealth: async () => ({ engine: { ready: true, healthy: true } }),
    sendJson: (res, status, body) => {
      sentStatus = status;
      sentBody = body;
    },
  });

  const fakeRes = { statusCode: 0, end: () => {}, destroy: () => {} };
  return handler({}, fakeRes).then(() => {
    assert.equal(sentStatus, 200);
    assert.equal(sentBody.aca_integration, false);
    assert.equal(sentBody.aca_reason, "aca_not_configured");
    assert.equal(sentBody.coding_workflows, true);
    assert.equal(sentBody.engine_healthy, true);
  });
});

test("ACA probe timeout increments timeout counter", () => {
  const metrics = getCapabilitiesMetrics();
  const before = metrics.aca_probe_error_counts.aca_probe_timeout;

  const handler = createCapabilitiesHandler({
    PROBE_TIMEOUT_MS: 10,
    ACA_BASE_URL: "http://127.0.0.1:59999",
    engineHealth: async () => ({ engine: { ready: true, healthy: true } }),
    sendJson: () => {},
  });

  const fakeRes = { statusCode: 0, end: () => {}, destroy: () => {} };
  return handler({}, fakeRes).then(() => {
    const after = getCapabilitiesMetrics().aca_probe_error_counts.aca_probe_timeout;
    assert.equal(after, before + 1);
  });
});

test("ACA 404 returns aca_endpoint_not_found and increments counter", async () => {
  const port = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      res.writeHead(404);
      res.end();
    });
    s.listen(0, "127.0.0.1", () => resolve(s));
  }).then((s) => {
    const addr = s.address();
    s.close();
    return addr.port;
  });

  const server = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      res.writeHead(404);
      res.end();
    });
    s.listen(0, "127.0.0.1", () => resolve(s));
  });
  const port2 = server.address().port;

  const metricsBefore = getCapabilitiesMetrics().aca_probe_error_counts.aca_endpoint_not_found;

  const handler = createCapabilitiesHandler({
    ACA_BASE_URL: `http://127.0.0.1:${port2}`,
    engineHealth: async () => ({ engine: { ready: true, healthy: true } }),
    sendJson: () => {},
  });

  const fakeRes = { statusCode: 0, end: () => {}, destroy: () => {} };
  await handler({}, fakeRes);
  server.close();

  const after = getCapabilitiesMetrics().aca_probe_error_counts.aca_endpoint_not_found;
  assert.equal(after, metricsBefore + 1);
});

test("ACA 503 returns aca_health_failed_xxx under wildcard bucket", async () => {
  const server = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      res.writeHead(503);
      res.end();
    });
    s.listen(0, "127.0.0.1", () => resolve(s));
  });
  const port = server.address().port;

  const handler = createCapabilitiesHandler({
    ACA_BASE_URL: `http://127.0.0.1:${port}`,
    engineHealth: async () => ({ engine: { ready: true, healthy: true } }),
    sendJson: () => {},
  });

  const fakeRes = { statusCode: 0, end: () => {}, destroy: () => {} };
  await handler({}, fakeRes);
  server.close();

  const m = getCapabilitiesMetrics();
  assert.equal(m.aca_probe_error_counts.aca_health_failed_xxx >= 1, true);
});

test("ACA available returns aca_integration true", async () => {
  const server = await new Promise((resolve) => {
    const s = createServer(async (req, res) => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ status: "ok" }));
    });
    s.listen(0, "127.0.0.1", () => resolve(s));
  });
  const port = server.address().port;

  let body = null;
  const handler = createCapabilitiesHandler({
    ACA_BASE_URL: `http://127.0.0.1:${port}`,
    engineHealth: async () => ({ engine: { ready: true, healthy: true } }),
    sendJson: (_, __, b) => { body = b; },
  });

  const fakeRes = { statusCode: 0, end: () => {}, destroy: () => {} };
  await handler({}, fakeRes);
  server.close();

  assert.equal(body.aca_integration, true);
  assert.equal(body.aca_reason, "");
  assert.equal(body.coding_workflows, true);
  assert.equal(body.missions, true);
  assert.equal(body.agent_teams, true);
  assert.equal(body.coder, true);
});

test("Engine unhealthy returns all coding features false", async () => {
  let body = null;
  const handler = createCapabilitiesHandler({
    ACA_BASE_URL: "",
    engineHealth: async () => null,
    sendJson: (_, __, b) => { body = b; },
  });

  const fakeRes = { statusCode: 0, end: () => {}, destroy: () => {} };
  await handler({}, fakeRes);

  assert.equal(body.engine_healthy, false, `expected false, got ${body?.engine_healthy}`);
  assert.equal(body.coding_workflows, false);
  assert.equal(body.missions, false);
  assert.equal(body.agent_teams, false);
  assert.equal(body.coder, false);
  assert.equal(body.aca_integration, false);
});

test("Cached response is returned without re-probing within TTL", async () => {
  let probeCount = 0;
  const handler = createCapabilitiesHandler({
    ACA_BASE_URL: "http://127.0.0.1:59999",
    engineHealth: async () => {
      probeCount += 1;
      return { engine: { ready: true, healthy: true } };
    },
    sendJson: () => {},
    cacheTtlMs: 10_000,
  });

  const fakeRes = { statusCode: 0, end: () => {}, destroy: () => {} };
  await handler({}, fakeRes);
  await handler({}, fakeRes);
  await handler({}, fakeRes);

  assert.equal(probeCount, 1);
});

test("getCapabilitiesMetrics returns structured metrics with error counts", () => {
  const m = getCapabilitiesMetrics();
  assert.equal(typeof m.detect_duration_ms, "number");
  assert.equal(typeof m.last_detect_at_ms, "number");
  assert.equal(typeof m.aca_probe_error_counts, "object");
  assert.ok("aca_not_configured" in m.aca_probe_error_counts);
  assert.ok("aca_endpoint_not_found" in m.aca_probe_error_counts);
  assert.ok("aca_probe_timeout" in m.aca_probe_error_counts);
  assert.ok("aca_probe_error" in m.aca_probe_error_counts);
  assert.ok("aca_health_failed_xxx" in m.aca_probe_error_counts);
});
