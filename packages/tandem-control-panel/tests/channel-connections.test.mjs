import { test } from "node:test";
import assert from "node:assert/strict";
import {
  connectionVerifyKey,
  ingressModeLabel,
  normalizeSlackConnections,
  normalizeSlackSenders,
  parseOrgUnitsInput,
  senderTone,
  unmappedBoundChannels,
  verifyResultsByChannel,
} from "../src/pages/channelConnectionsModel.mjs";

test("normalizeSlackConnections maps config summary rows and drops blanks", () => {
  const rows = normalizeSlackConnections({
    slack: {
      connections_summary: [
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
  assert.deepEqual(rows[0].channelAccess, []);
});

test("normalizeSlackSenders keeps per-channel access rows", () => {
  const rows = normalizeSlackSenders({
    senders: [
      {
        user_id: "U1",
        principal: "channel:slack:T1:A1:U1",
        mapped: false,
        org_units: ["department/engineering"],
        channel_access: [
          {
            channel_id: "C_SALES",
            bound_org_units: ["sales"],
            mapped: false,
            configured: true,
          },
          { channel_id: "C_ENG", bound_org_units: ["engineering"], mapped: true },
          { channel_id: "" },
        ],
      },
    ],
  });
  assert.equal(rows[0].channelAccess.length, 2);
  assert.deepEqual(rows[0].channelAccess[0], {
    channelId: "C_SALES",
    boundOrgUnits: ["sales"],
    mapped: false,
    configured: true,
  });
  assert.equal(rows[0].channelAccess[1].configured, true);
});

test("unmappedBoundChannels surfaces only actionable department gaps", () => {
  const gaps = unmappedBoundChannels({
    channelAccess: [
      { channelId: "C_SALES", boundOrgUnits: ["sales"], mapped: false, configured: true },
      { channelId: "C_ENG", boundOrgUnits: ["engineering"], mapped: true, configured: true },
      { channelId: "C_OPEN", boundOrgUnits: [], mapped: false, configured: true },
      { channelId: "C_GONE", boundOrgUnits: ["ops"], mapped: false, configured: false },
    ],
  });
  assert.deepEqual(
    gaps.map((entry) => entry.channelId),
    ["C_SALES"],
  );
  assert.deepEqual(unmappedBoundChannels({}), []);
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
  const ok = byChannel.get(connectionVerifyKey({ channelId: "C_OK" }));
  const bad = byChannel.get(connectionVerifyKey({ channelId: "C_BAD" }));
  assert.equal(ok.ok, true);
  assert.equal(bad.ok, false);
  assert.equal(bad.teamOk, false);
  assert.match(bad.error, /T2/);
});

test("verifyResultsByChannel keeps colliding channel ids apart by installation", () => {
  const byConnection = verifyResultsByChannel({
    connections: [
      { channel_id: "C_SHARED", team_id: "T_A", app_id: "A_A", ok: true, token_ok: true },
      {
        channel_id: "C_SHARED",
        team_id: "T_B",
        app_id: "A_B",
        ok: false,
        error: "bot token belongs to a different Slack app",
      },
    ],
  });
  assert.equal(byConnection.size, 2, "one row per installation must survive");
  const a = byConnection.get(
    connectionVerifyKey({ teamId: "T_A", appId: "A_A", channelId: "C_SHARED" }),
  );
  const b = byConnection.get(
    connectionVerifyKey({ teamId: "T_B", appId: "A_B", channelId: "C_SHARED" }),
  );
  assert.equal(a.ok, true);
  assert.equal(b.ok, false);
  assert.match(b.error, /different Slack app/);
});

test("parseOrgUnitsInput trims, drops empties, and dedups", () => {
  assert.deepEqual(parseOrgUnitsInput(" department/sales, engineering ,, department/sales "), [
    "department/sales",
    "engineering",
  ]);
  assert.deepEqual(parseOrgUnitsInput(""), []);
  assert.deepEqual(parseOrgUnitsInput(undefined), []);
});
