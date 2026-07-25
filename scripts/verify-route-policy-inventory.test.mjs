import test from "node:test";
import assert from "node:assert/strict";

import {
  assertLoopbackCallbackAddress,
  classifyRoute,
  extractRoutesFromRust,
  verifyInventory,
} from "./verify-route-policy-inventory.mjs";

test("extracts every method from a chained Axum route", () => {
  const source = `
    fn apply(router: Router<AppState>) -> Router<AppState> {
      router.route("/resource/{id}", get(read).put(replace).delete(remove))
    }
  `;
  const result = extractRoutesFromRust(source, "routes_resources.rs");
  assert.deepEqual(
    result.routes.map((route) => `${route.method} ${route.path}`).sort(),
    ["DELETE /resource/{id}", "GET /resource/{id}", "HEAD /resource/{id}", "PUT /resource/{id}"],
  );
  assert.deepEqual(result.unsupported, []);
});

test("classifies every automation webhook spelling as public capability ingress", () => {
  for (const routePath of [
    "/webhooks/automations/{public_path_token}",
    "/api/engine/webhooks/automations/{public_path_token}/{setup_nonce}",
  ]) {
    const policy = classifyRoute({
      listener: "engine_api",
      method: "POST",
      path: routePath,
    });
    assert.equal(policy.ingress_policy, "public_capability_webhook");
    assert.equal(policy.capability, "automation.webhook.trigger");
  }
});

test("keeps signed channel interactions behind runtime auth plus signature policy", () => {
  const policy = classifyRoute({
    listener: "engine_api",
    method: "POST",
    path: "/channels/discord/interactions",
  });
  assert.equal(policy.ingress_policy, "runtime_auth_gate");
  assert.equal(
    policy.authorization_policy,
    "channel_signature_identity_and_tenant_binding",
  );
});

test("ignores test-only routers", () => {
  const source = `
    fn apply(router: Router<AppState>) -> Router<AppState> {
      router.route("/production", post(handler))
    }
    #[cfg(test)]
    mod tests {
      fn fixture() { Router::new().route("/fixture", get(handler)); }
    }
  `;
  const result = extractRoutesFromRust(source, "routes_fixture.rs");
  assert.deepEqual(
    result.routes.map((route) => `${route.method} ${route.path}`).sort(),
    ["POST /production"],
  );
});

test("reports dynamic paths instead of silently omitting them", () => {
  const source = `fn apply(router: Router<AppState>, path: &str) { router.route(path, get(handler)); }`;
  const result = extractRoutesFromRust(source, "routes_dynamic.rs");
  assert.equal(result.routes.length, 0);
  assert.equal(result.unsupported.length, 1);
  assert.match(result.unsupported[0], /dynamic route path/);
});

test("reports computed incident-monitor suffixes instead of silently omitting them", () => {
  const source = `
    fn apply(router: Router<AppState>, suffix: &str) -> Router<AppState> {
      route_prefixed(router, "/api/engine", suffix, get(handler))
    }
  `;
  const result = extractRoutesFromRust(source, "routes_incident_monitor.rs");
  assert.equal(result.routes.length, 0);
  assert.equal(result.unsupported.length, 1);
  assert.match(result.unsupported[0], /dynamic incident-monitor suffix/);
});

test("rejects a wildcard OAuth callback bind even when the constant name is unchanged", () => {
  assert.throws(
    () =>
      assertLoopbackCallbackAddress(
        'const OPENAI_CODEX_LOCAL_CALLBACK_ADDR: &str = "0.0.0.0:1455";',
      ),
    /not statically loopback-only/,
  );
  assert.doesNotThrow(() =>
    assertLoopbackCallbackAddress(
      'const OPENAI_CODEX_LOCAL_CALLBACK_ADDR: &str = "127.0.0.1:1455";',
    ),
  );
});

test("classifies effective config mutation and protected audit policies", () => {
  const config = classifyRoute({
    listener: "engine_api",
    method: "PATCH",
    path: "/config",
  });
  assert.equal(config.policy_origin, "admin.deployment");
  assert.equal(config.capability, "deployment.config.manage");

  for (const path of [
    "/audit/protected",
    "/audit/stream",
    "/audit/ledger/manifest",
    "/audit/ledger/export",
  ]) {
    const audit = classifyRoute({ listener: "engine_api", method: "GET", path });
    assert.equal(audit.policy_origin, "audit.admin");
    assert.equal(
      audit.authorization_policy,
      "api_token_or_control_panel_admin_source_and_tenant_scope",
    );
  }
});

test("rejects a stale committed inventory", () => {
  assert.throws(
    () => verifyInventory("{}\n", { schema_version: 1, policies: [] }),
    /inventory drifted/,
  );
});
