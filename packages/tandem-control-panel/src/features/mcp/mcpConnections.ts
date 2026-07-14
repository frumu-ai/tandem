import type { StudioMcpConnectionGrant } from "../studio/schema";

export type McpConnectionClass =
  | "user_owned"
  | "service_account"
  | "shared_read_only"
  | "shared_read_write"
  | "admin_managed"
  | "unknown";

export type McpConnectionSummary = {
  connectionId: string;
  connectionGeneration: string;
  server: string;
  connectionClass: McpConnectionClass;
  connected: boolean;
  enabled: boolean;
  owner: Record<string, any> | null;
  tenantContext: Record<string, any> | null;
  upstreamAccount: Record<string, any> | null;
  lastError: string;
  pendingAuth: boolean;
  authorizationUrl: string;
  toolCache: string[];
  toolCount: number;
  oauthProviderId: string;
  localImplicit: boolean;
  createdAtMs: number;
  updatedAtMs: number;
};

export type McpInventoryServerRow = Record<string, any> & {
  name: string;
};

function safeString(value: unknown) {
  return String(value || "").trim();
}

function safeObject(value: unknown): Record<string, any> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, any>)
    : null;
}

function safeNumber(value: unknown) {
  const number = Number(value || 0);
  return Number.isFinite(number) ? number : 0;
}

function normalizeConnectionClass(value: unknown): McpConnectionClass {
  const raw = safeString(value)
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[-\s]+/g, "_")
    .toLowerCase();
  if (
    raw === "user_owned" ||
    raw === "service_account" ||
    raw === "shared_read_only" ||
    raw === "shared_read_write" ||
    raw === "admin_managed"
  ) {
    return raw;
  }
  return "unknown";
}

export function normalizeMcpConnectionSummary(
  row: unknown,
  fallbackServer = ""
): McpConnectionSummary | null {
  const record = safeObject(row);
  if (!record) return null;
  const server = safeString(
    record.server || record.server_id || record.serverId || record.serverName || fallbackServer
  );
  const connectionId = safeString(record.connection_id || record.connectionId || record.id);
  if (!server || !connectionId) return null;
  const lastAuthChallenge = safeObject(record.last_auth_challenge || record.lastAuthChallenge);
  const authorizationUrl = safeString(
    lastAuthChallenge?.authorization_url ||
      lastAuthChallenge?.authorizationUrl ||
      record.authorization_url ||
      record.authorizationUrl
  );
  const tenantContext = safeObject(record.tenant_context || record.tenantContext);
  return {
    connectionId,
    connectionGeneration: safeString(record.connection_generation || record.connectionGeneration),
    server,
    connectionClass: normalizeConnectionClass(record.connection_class || record.connectionClass),
    connected: !!record.connected,
    enabled: record.enabled !== false,
    owner: safeObject(record.owner || record.principal),
    tenantContext,
    upstreamAccount: safeObject(record.upstream_account || record.upstreamAccount),
    lastError: safeString(record.last_error || record.lastError),
    pendingAuth: !!lastAuthChallenge || !!authorizationUrl,
    authorizationUrl,
    toolCache: normalizeConnectionToolNames(record.tool_cache || record.toolCache),
    toolCount: safeNumber(
      record.tool_count ||
        record.toolCount ||
        (Array.isArray(record.tool_cache || record.toolCache)
          ? (record.tool_cache || record.toolCache).length
          : 0)
    ),
    oauthProviderId: safeString(record.oauth_provider_id || record.oauthProviderId),
    localImplicit:
      !!record.local_implicit ||
      !!record.localImplicit ||
      safeString(tenantContext?.source).toLowerCase() === "localimplicit" ||
      safeString(tenantContext?.source).toLowerCase() === "local_implicit",
    createdAtMs: safeNumber(record.created_at_ms || record.createdAtMs),
    updatedAtMs: safeNumber(record.updated_at_ms || record.updatedAtMs),
  };
}

function normalizeConnectionToolNames(raw: unknown) {
  const rows = Array.isArray(raw) ? raw : [];
  const seen = new Set<string>();
  const names: string[] = [];
  for (const row of rows) {
    const record = safeObject(row);
    const name =
      typeof row === "string"
        ? safeString(row)
        : safeString(
            record?.namespaced_name ||
              record?.namespacedName ||
              record?.tool_name ||
              record?.toolName ||
              record?.name
          );
    if (!name || seen.has(name)) continue;
    seen.add(name);
    names.push(name);
  }
  return names.sort();
}

export function normalizeMcpConnectionSummaries(
  raw: unknown,
  fallbackServer = ""
): McpConnectionSummary[] {
  const rows = Array.isArray(raw) ? raw : [];
  const seen = new Set<string>();
  const connections: McpConnectionSummary[] = [];
  for (const row of rows) {
    const connection = normalizeMcpConnectionSummary(row, fallbackServer);
    if (!connection || seen.has(connection.connectionId)) continue;
    seen.add(connection.connectionId);
    connections.push(connection);
  }
  return connections.sort(compareMcpConnections);
}

export function normalizeMcpConnectionsFromInventory(raw: unknown): McpConnectionSummary[] {
  const rows: McpConnectionSummary[] = [];
  const seen = new Set<string>();
  const add = (connection: McpConnectionSummary | null) => {
    if (!connection || seen.has(connection.connectionId)) return;
    seen.add(connection.connectionId);
    rows.push(connection);
  };

  const record = safeObject(raw);
  if (Array.isArray((record as any)?.connections)) {
    for (const connection of (record as any).connections)
      add(normalizeMcpConnectionSummary(connection));
  }
  if (Array.isArray((record as any)?.servers)) {
    for (const serverRow of (record as any).servers) {
      const server = safeString((serverRow as any)?.name);
      for (const connection of normalizeMcpConnectionSummaries(
        (serverRow as any)?.connections,
        server
      )) {
        add(connection);
      }
    }
  } else if (record) {
    for (const [server, serverRow] of Object.entries(record)) {
      for (const connection of normalizeMcpConnectionSummaries(
        (serverRow as any)?.connections,
        server
      )) {
        add(connection);
      }
    }
  }

  return rows.sort(compareMcpConnections);
}

export function normalizeMcpInventoryServerRows(raw: unknown): McpInventoryServerRow[] {
  const record = safeObject(raw);
  if (Array.isArray((record as any)?.servers)) return (record as any).servers;
  if (!record) return [];
  return Object.entries(record).map(([name, row]) => ({
    ...((row && typeof row === "object" ? row : {}) as Record<string, any>),
    name,
  }));
}

export function compareMcpConnections(left: McpConnectionSummary, right: McpConnectionSummary) {
  return (
    left.server.localeCompare(right.server) ||
    mcpConnectionOwnerLabel(left).localeCompare(mcpConnectionOwnerLabel(right)) ||
    left.connectionId.localeCompare(right.connectionId)
  );
}

export function mcpConnectionClassLabel(connection: McpConnectionSummary) {
  switch (connection.connectionClass) {
    case "user_owned":
      return connection.localImplicit ? "Local account" : "My account";
    case "service_account":
      return "Service account";
    case "shared_read_only":
      return "Shared read-only";
    case "shared_read_write":
      return "Shared read-write";
    case "admin_managed":
      return "Admin-managed";
    default:
      return "Connection";
  }
}

export function mcpConnectionClassTone(connection: McpConnectionSummary) {
  if (!connection.enabled || !connection.connected) return "tcp-badge-warn";
  if (connection.connectionClass === "user_owned") return "tcp-badge-ok";
  if (connection.connectionClass === "service_account") return "tcp-badge-blocked";
  return "tcp-badge-info";
}

export function mcpConnectionOwnerLabel(connection: McpConnectionSummary) {
  const upstream = connection.upstreamAccount || {};
  const upstreamLabel = safeString(
    upstream.display_name || upstream.displayName || upstream.email || upstream.account_id
  );
  if (upstreamLabel) return upstreamLabel;
  const owner = connection.owner || {};
  const ownerType = safeString(owner.type || owner.kind);
  if (ownerType === "human_actor" || owner.actor_id || owner.actorId) {
    return safeString(owner.actor_id || owner.actorId) || "Human actor";
  }
  if (ownerType === "service_principal" || owner.principal_id || owner.principalId) {
    return safeString(owner.principal_id || owner.principalId) || "Service principal";
  }
  if (ownerType === "automation_principal" || owner.automation_id || owner.automationId) {
    return safeString(owner.automation_id || owner.automationId) || "Automation principal";
  }
  if (ownerType === "shared_connection" || owner.grant_id || owner.grantId) {
    return safeString(owner.grant_id || owner.grantId) || "Shared grant";
  }
  if (connection.localImplicit) return "Local operator";
  return "Unattributed";
}

export function mcpConnectionScopeLabel(connection: McpConnectionSummary) {
  const tenant = connection.tenantContext || {};
  if (connection.localImplicit) return "local";
  const org = safeString(tenant.org_id || tenant.orgId);
  const workspace = safeString(tenant.workspace_id || tenant.workspaceId);
  const actor = safeString(tenant.actor_id || tenant.actorId);
  return [org, workspace, actor].filter(Boolean).join(" / ") || "tenant scoped";
}

export function mcpConnectionGrantFor(connection: McpConnectionSummary): StudioMcpConnectionGrant {
  return {
    server: connection.server,
    connection_id: connection.connectionId,
    connection_generation: connection.connectionGeneration,
  };
}

export function mcpConnectionGrantKey(grant: StudioMcpConnectionGrant) {
  return JSON.stringify({
    server: safeString(grant.server),
    connection_id: safeString(grant.connection_id),
    connection_generation: safeString(grant.connection_generation),
    run_as: grant.run_as || null,
  });
}

export function mcpConnectionGrantIdentityKey(grant: StudioMcpConnectionGrant) {
  return JSON.stringify({
    server: safeString(grant.server).toLowerCase(),
    connection_id: safeString(grant.connection_id),
    connection_generation: safeString(grant.connection_generation),
  });
}

export function normalizeMcpConnectionGrants(raw: unknown): StudioMcpConnectionGrant[] {
  const rows = Array.isArray(raw) ? raw : [];
  const seen = new Set<string>();
  const grants: StudioMcpConnectionGrant[] = [];
  for (const row of rows) {
    const record = safeObject(row);
    if (!record) continue;
    const server = safeString(record.server || record.server_name || record.serverName);
    if (!server) continue;
    const connectionId = safeString(record.connection_id || record.connectionId);
    const connectionGeneration = safeString(
      record.connection_generation || record.connectionGeneration
    );
    const runAs = record.run_as ?? record.runAs;
    const grant: StudioMcpConnectionGrant = {
      server,
      ...(connectionId ? { connection_id: connectionId } : {}),
      ...(connectionGeneration ? { connection_generation: connectionGeneration } : {}),
      ...(runAs && typeof runAs === "object" ? { run_as: JSON.parse(JSON.stringify(runAs)) } : {}),
    };
    const key = mcpConnectionGrantKey(grant);
    if (seen.has(key)) continue;
    seen.add(key);
    grants.push(grant);
  }
  return grants;
}

export function mcpConnectionStatusLabel(connection: McpConnectionSummary) {
  if (!connection.enabled) return "Disabled";
  if (connection.connected) return "Connected";
  if (connection.pendingAuth) return "Sign-in pending";
  return "Disconnected";
}
