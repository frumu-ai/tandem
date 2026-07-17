// Pure view-model helpers for the Channel Connections page (TAN-766).
// Kept framework-free so tests/channel-connections.test.mjs can exercise the
// normalization logic without React.

/**
 * Normalize the `slack.connections_summary` rows from `GET /channels/config`
 * (per-connection presence flags added in TAN-763). The summary key is
 * deliberately distinct from the real `connections` config key so clients
 * echoing the snapshot back through PUT can't clobber connection config.
 */
export function normalizeSlackConnections(config) {
  const slack = config && typeof config === "object" ? config.slack : null;
  const rows =
    slack && Array.isArray(slack.connections_summary) ? slack.connections_summary : [];
  return rows
    .filter((row) => row && typeof row === "object")
    .map((row) => ({
      channelId: row.channel_id ? String(row.channel_id) : "",
      teamId: row.team_id ? String(row.team_id) : "",
      appId: row.app_id ? String(row.app_id) : "",
      hasToken: Boolean(row.has_token),
      hasSigningSecret: Boolean(row.has_signing_secret),
      eventsEnabled: Boolean(row.events_enabled),
      eventsCapable: Boolean(row.events_capable),
      mentionOnly: Boolean(row.mention_only),
      notifyApprovals: row.notify_approvals !== false,
      tenantOrgId: row.tenant_org_id ? String(row.tenant_org_id) : "",
      tenantWorkspaceId: row.tenant_workspace_id ? String(row.tenant_workspace_id) : "",
      orgUnits: Array.isArray(row.org_units)
        ? row.org_units.map((unit) => String(unit)).filter(Boolean)
        : [],
    }))
    .filter((row) => row.channelId);
}

/**
 * How this connection ingests messages. Signed Events is the governed path;
 * a connection with events enabled but no signing secret cannot serve it.
 */
export function ingressModeLabel(connection) {
  if (connection.eventsCapable) return "Signed events";
  if (connection.eventsEnabled) return "Events (signing secret missing)";
  return "Legacy poller";
}

/** Normalize `GET /channels/slack/senders` rows (TAN-765). */
export function normalizeSlackSenders(payload) {
  const rows = payload && Array.isArray(payload.senders) ? payload.senders : [];
  return rows
    .filter((row) => row && typeof row === "object")
    .map((row) => ({
      userId: row.user_id ? String(row.user_id) : "",
      teamId: row.team_id ? String(row.team_id) : "",
      appId: row.app_id ? String(row.app_id) : "",
      principal: row.principal ? String(row.principal) : "",
      channels: Array.isArray(row.channels)
        ? row.channels.map((channel) => String(channel)).filter(Boolean)
        : [],
      acceptedCount: Number(row.accepted_count) || 0,
      deniedCount: Number(row.denied_count) || 0,
      lastSeenAtMs: Number(row.last_seen_at_ms) || 0,
      lastDenialReason: row.last_denial_reason ? String(row.last_denial_reason) : "",
      mapped: Boolean(row.mapped),
      orgUnits: Array.isArray(row.org_units)
        ? row.org_units.map((unit) => String(unit)).filter(Boolean)
        : [],
      channelAccess: Array.isArray(row.channel_access)
        ? row.channel_access
            .filter((entry) => entry && typeof entry === "object" && entry.channel_id)
            .map((entry) => ({
              channelId: String(entry.channel_id),
              boundOrgUnits: Array.isArray(entry.bound_org_units)
                ? entry.bound_org_units.map((unit) => String(unit)).filter(Boolean)
                : [],
              mapped: Boolean(entry.mapped),
              configured: entry.configured !== false,
            }))
        : [],
      tenantOrgId: row.tenant_org_id ? String(row.tenant_org_id) : "",
      tenantWorkspaceId: row.tenant_workspace_id ? String(row.tenant_workspace_id) : "",
    }))
    .filter((row) => row.principal);
}

/**
 * Row identity for one sender row. The Slack principal alone is NOT unique:
 * the senders endpoint returns one row per bound tenant, and the same
 * installation/user can appear under several tenants — keying (or matching
 * the enrollment target) by principal alone would reconcile those rows into
 * each other and let a pairing-code editor opened for one tenant shadow the
 * other.
 */
export function senderRowKey(sender) {
  const orgId = sender && sender.tenantOrgId ? String(sender.tenantOrgId) : "";
  const workspaceId = sender && sender.tenantWorkspaceId ? String(sender.tenantWorkspaceId) : "";
  const principal = sender && sender.principal ? String(sender.principal) : "";
  return `${orgId} ${workspaceId} ${principal}`;
}

/** Badge tone for a sender: mapped=ok, denied-and-unmapped=err, else warn. */
export function senderTone(sender) {
  if (sender.mapped) return "ok";
  if (sender.deniedCount > 0) return "err";
  return "warn";
}

/**
 * Which department-bound channels this sender is still locked out of —
 * the actionable gap behind an unmapped badge (TAN-765). Each row names the
 * channel and the units an admin could map the sender into.
 */
export function unmappedBoundChannels(sender) {
  const rows = Array.isArray(sender && sender.channelAccess) ? sender.channelAccess : [];
  return rows.filter(
    (entry) => entry.configured && !entry.mapped && entry.boundOrgUnits.length > 0,
  );
}

/**
 * Composite key for one connection's verify result. Two installations can
 * share a channel-id string, so verify rows must be keyed by the full
 * `(team, app, channel)` binding or one row silently overwrites the other.
 */
export function connectionVerifyKey(connection) {
  const teamId = connection && connection.teamId ? String(connection.teamId) : "";
  const appId = connection && connection.appId ? String(connection.appId) : "";
  const channelId = connection && connection.channelId ? String(connection.channelId) : "";
  return `${teamId}\u0000${appId}\u0000${channelId}`;
}

/** Index `POST /channels/slack/verify` rows by their installation binding. */
export function verifyResultsByChannel(payload) {
  const rows = payload && Array.isArray(payload.connections) ? payload.connections : [];
  const byConnection = new Map();
  for (const row of rows) {
    if (!row || typeof row !== "object" || !row.channel_id) continue;
    const key = connectionVerifyKey({
      teamId: row.team_id,
      appId: row.app_id,
      channelId: row.channel_id,
    });
    byConnection.set(key, {
      ok: row.ok === true,
      error: row.error ? String(row.error) : "",
      tokenOk: row.token_ok === true,
      teamOk: row.team_ok !== false,
      appOk: row.app_ok !== false,
    });
  }
  return byConnection;
}

/**
 * Slack allowlist input → FAITHFUL list. A blank field stays empty
 * (deny-all on signed ingress) instead of being normalized to a "*"
 * wildcard — a routine settings save must never silently flip a deny-all
 * channel to open-to-all. Opening a channel to everyone requires the
 * operator explicitly typing `*`.
 */
export function parseSlackAllowedUsers(input) {
  return String(input || "")
    .split(",")
    .map((row) => row.trim())
    .filter(Boolean);
}

/** Comma-separated org-unit input → trimmed, deduped list. */
export function parseOrgUnitsInput(raw) {
  const seen = new Set();
  const out = [];
  for (const entry of String(raw || "").split(",")) {
    const normalized = entry.trim();
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}
