import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ingressModeLabel,
  normalizeSlackConnections,
  normalizeSlackSenders,
  parseOrgUnitsInput,
  senderTone,
  verifyResultsByChannel,
} from "../src/pages/channelConnectionsModel.mjs";

test("normalizeSlackConnections maps config summary rows and drops blanks", () => {
  const rows = normalizeSlackConnections({
    slack: {
      connections: [
        {
          channel_id: "C_SALES",
          team_id: "T1",
          app_id: "A1",
          has_token: true,
          has_signing_secret: true,
          events_enabled: true,
          events_capable: true,
          mention_only: false,
          notify_approvals: false,
          tenant_org_id: "acme",
          tenant_workspace_id: "hq",
          org_units: ["department/sales"],
        },
        { channel_id: "" },
        null,
      ],
    },
  });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].channelId, "C_SALES");
  assert.equal(rows[0].eventsCapable, true);
  assert.equal(rows[0].notifyApprovals, false);
  assert.deepEqual(rows[0].orgUnits, ["department/sales"]);
});

test("normalizeSlackConnections tolerates missing config", () => {
  assert.deepEqual(normalizeSlackConnections(undefined), []);
  assert.deepEqual(normalizeSlackConnections({}), []);
  assert.deepEqual(normalizeSlackConnections({ slack: {} }), []);
});

test("ingressModeLabel distinguishes signed events, misconfigured events, and poller", () => {
  assert.equal(ingressModeLabel({ eventsCapable: true, eventsEnabled: true }), "Signed events");
  assert.equal(
    ingressModeLabel({ eventsCapable: false, eventsEnabled: true }),
    "Events (signing secret missing)"
  );
  assert.equal(ingressModeLabel({ eventsCapable: false, eventsEnabled: false }), "Legacy poller");
});

test("normalizeSlackSenders keeps principals and counts", () => {
  const rows = normalizeSlackSenders({
    senders: [
      {
        user_id: "U1",
        team_id: "T1",
        app_id: "A1",
        principal: "channel:slack:T1:A1:U1",
        channels: ["C_SALES"],
        accepted_count: 2,
        denied_count: 1,
        last_seen_at_ms: 42,
        last_denial_reason: "Slack user has no active organization-unit membership",
        mapped: false,
        org_units: [],
      },
      { user_id: "U2" },
    ],
  });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].principal, "channel:slack:T1:A1:U1");
  assert.equal(rows[0].acceptedCount, 2);
  assert.equal(rows[0].deniedCount, 1);
  assert.match(rows[0].lastDenialReason, /organization-unit membership/);
});

test("senderTone flags unmapped denials as errors", () => {
  assert.equal(senderTone({ mapped: true, deniedCount: 3 }), "ok");
  assert.equal(senderTone({ mapped: false, deniedCount: 1 }), "err");
  assert.equal(senderTone({ mapped: false, deniedCount: 0 }), "warn");
});

test("verifyResultsByChannel indexes verify rows", () => {
  const byChannel = verifyResultsByChannel({
    connections: [
      { channel_id: "C_OK", ok: true, token_ok: true, team_ok: true, app_ok: true },
      { channel_id: "C_BAD", ok: false, team_ok: false, error: "bot token belongs to team T2" },
    ],
  });
  assert.equal(byChannel.get("C_OK").ok, true);
  assert.equal(byChannel.get("C_BAD").ok, false);
  assert.equal(byChannel.get("C_BAD").teamOk, false);
  assert.match(byChannel.get("C_BAD").error, /T2/);
});

test("parseOrgUnitsInput trims, drops empties, and dedups", () => {
  assert.deepEqual(parseOrgUnitsInput(" department/sales, engineering ,, department/sales "), [
    "department/sales",
    "engineering",
  ]);
  assert.deepEqual(parseOrgUnitsInput(""), []);
  assert.deepEqual(parseOrgUnitsInput(undefined), []);
});
