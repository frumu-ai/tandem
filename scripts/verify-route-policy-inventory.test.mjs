import test from "node:test";
import assert from "node:assert/strict";

import {
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

test("rejects a stale committed inventory", () => {
  assert.throws(
    () => verifyInventory("{}\n", { schema_version: 1, policies: [] }),
    /inventory drifted/,
  );
});
