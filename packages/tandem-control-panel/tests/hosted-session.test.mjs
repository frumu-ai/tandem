import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { hostedSessionFields, hostedSessionExpired, assertSameHostedIdentity, hostedPanelRouteAllowed } from "../lib/setup/hosted-session.js";
import { isEngineIdentityHeader, sessionEngineHeaders } from "../lib/setup/engine-identity-headers.js";

const now = Date.parse("2026-09-04T00:00:00Z");
const payload = () => ({
  deployment_id: "deployment-a", user: { id: "user-a" },
  panel_session_token: "server-only-session", context_assertion: "server-only-assertion",
  context_assertion_expires_at: new Date(now + 300_000).toISOString(),
  session_expires_at: new Date(now + 3_600_000).toISOString(),
  role: "member", org_units: ["engineering"], policy_version: 1,
});

test("hosted envelope requires current identity, deployment and expiration", () => {
  const options = { deploymentId: "deployment-a", now };
  const session = hostedSessionFields(payload(), options);
  assert.equal(session.principal_id, "user-a");
  assert.equal(session.principal_scope, "deployment-a");
  for (const field of ["user", "deployment_id", "panel_session_token", "context_assertion", "context_assertion_expires_at", "session_expires_at"]) {
    const incomplete = payload();
    delete incomplete[field];
    assert.throws(() => hostedSessionFields(incomplete, options), /current user/);
  }
  assert.throws(() => hostedSessionFields(payload(), { deploymentId: "deployment-b", now }), /current user/);
  for (const field of ["context_assertion_expires_at", "session_expires_at"]) {
    assert.throws(() => hostedSessionFields({ ...payload(), [field]: new Date(now).toISOString() }, options), /current user/);
  }
});

test("refresh permits credential and membership rotation but cannot change identity", () => {
  const options = { deploymentId: "deployment-a", now };
  const previous = hostedSessionFields(payload(), options);
  const next = hostedSessionFields({ ...payload(), panel_session_token: "rotated", context_assertion: "rotated-assertion", org_units: ["finance"], policy_version: 2 }, options);
  assert.doesNotThrow(() => assertSameHostedIdentity(previous, next));
  assert.throws(() => assertSameHostedIdentity(previous, { ...next, principal_id: "user-b" }), /changed user/);
  assert.throws(() => assertSameHostedIdentity(previous, { ...next, principal_scope: "deployment-b" }), /changed user/);
});

test("absolute hosted session expiry cannot be extended by activity", () => {
  const session = hostedSessionFields(payload(), { deploymentId: "deployment-a", now });
  assert.equal(hostedSessionExpired(session, now), false);
  assert.equal(hostedSessionExpired({ ...session, lastSeenAt: now + 3_600_000 }, now + 3_600_000), true);
  assert.equal(hostedSessionExpired({ ...session, session_expires_at: "invalid" }, now), true);
  assert.equal(hostedSessionExpired({ token: "local-operator" }, now), false);
});

test("deployment-wide handlers require hosted administration without restricting personal preferences", () => {
  const member = hostedSessionFields(payload(), { deploymentId: "deployment-a", now });
  for (const path of ["/api/control-panel/config", "/api/files/upload", "/api/workspace/files", "/api/knowledgebase/admin/collections", "/api/knowledgebase-suffix", "/api/aca/run", "/api/swarm/start", "/api/orchestrator", "/api/system/search-settings/test", "/api/system/scheduler-settings"]) {
    assert.equal(hostedPanelRouteAllowed(member, path), false, path);
    assert.equal(hostedPanelRouteAllowed({ ...member, hosted_role: "owner" }, path), true, path);
    assert.equal(hostedPanelRouteAllowed({ ...member, hosted_capabilities: ["hosted.admin"] }, path), true, path);
    assert.equal(hostedPanelRouteAllowed({ token: "local-operator" }, path), true, path);
  }
  assert.equal(hostedPanelRouteAllowed(member, "/api/control-panel/preferences"), true);
  assert.equal(hostedPanelRouteAllowed(member, "/api/engine/memory/search"), true);
});

test("server session headers override credentials and all browser identity aliases", () => {
  const extras = {
    authorization: "Bearer forged", "x-tandem-token": "forged",
    "X-Tandem-Context-Assertion": "forged", "x-tandem-context-jws": "forged",
    "x-tandem-tenant-context-jws": "forged", "x-user-id": "other-user",
    "x-tenant-org-id": "other-org", "x-tenant-workspace-id": "other-workspace",
    "x-tandem-actor-id": "other-actor", "x-tandem-roles": "admin",
    "x-tandem-agent-id": "other-agent", "x-tandem-request-source": "agent",
    "content-type": "application/json",
  };
  const headers = sessionEngineHeaders({ token: "transport-token", context_assertion: "verified-context" }, extras);
  assert.equal(headers.get("authorization"), "Bearer transport-token");
  assert.equal(headers.get("x-tandem-token"), "transport-token");
  assert.equal(headers.get("x-tandem-context-assertion"), "verified-context");
  for (const name of Object.keys(extras)) {
    if (isEngineIdentityHeader(name) && name.toLowerCase() !== "x-tandem-context-assertion") assert.equal(headers.get(name), null);
  }
  assert.equal(headers.get("content-type"), "application/json");
  const local = sessionEngineHeaders({ token: "local-token" }, extras);
  assert.equal(local.get("x-tandem-context-assertion"), null);
});

test("newly scaffolded panels keep the same identity-header boundary", async () => {
  const source = await readFile(new URL("../lib/setup/engine-identity-headers.js", import.meta.url), "utf8");
  const scaffold = await readFile(new URL("../../create-tandem-panel/template/lib/setup/engine-identity-headers.js", import.meta.url), "utf8");
  assert.equal(scaffold, source);
});
