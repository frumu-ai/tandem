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

/** Index `POST /channels/slack/verify` rows by channel id. */
export function verifyResultsByChannel(payload) {
  const rows = payload && Array.isArray(payload.connections) ? payload.connections : [];
  const byChannel = new Map();
  for (const row of rows) {
    if (!row || typeof row !== "object" || !row.channel_id) continue;
    byChannel.set(String(row.channel_id), {
      ok: row.ok === true,
      error: row.error ? String(row.error) : "",
      tokenOk: row.token_ok === true,
      teamOk: row.team_ok !== false,
      appOk: row.app_ok !== false,
    });
  }
  return byChannel;
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
