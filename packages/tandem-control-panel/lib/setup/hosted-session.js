// Validates the trusted control-plane response envelope. Signature/replay and
// authorization verification still belong to the engine's existing verifier.
// This module must not accept a browser-provided user/profile/assertion payload.
export function hostedSessionExpired(session, now = Date.now()) {
  if (session?.hosted !== true) return false;
  const expiry = Date.parse(String(session.session_expires_at || ""));
  return !Number.isFinite(expiry) || expiry <= now;
}

export function hostedSessionFields(payload, { deploymentId, now = Date.now() } = {}) {
  const assertionExpiry = Date.parse(String(payload?.context_assertion_expires_at || ""));
  const sessionExpiry = Date.parse(String(payload?.session_expires_at || ""));
  const userId = String(payload?.user?.id || "").trim();
  const payloadDeployment = String(payload?.deployment_id || "").trim();
  const assertion = String(payload?.context_assertion || "").trim();
  const sessionToken = String(payload?.panel_session_token || "").trim();
  if (!deploymentId || payloadDeployment !== deploymentId || !userId || !assertion ||
      !sessionToken || !Number.isFinite(assertionExpiry) || assertionExpiry <= now ||
      !Number.isFinite(sessionExpiry) || sessionExpiry <= now) {
    throw new Error("Hosted login did not return a current user, deployment and verified-session envelope.");
  }
  return {
    hosted: true,
    panel_session_token: sessionToken,
    context_assertion: assertion,
    context_assertion_expires_at: String(payload.context_assertion_expires_at),
    session_expires_at: String(payload.session_expires_at),
    hosted_role: String(payload?.role || ""),
    hosted_roles: Array.isArray(payload?.roles) ? payload.roles.map(String) : [],
    hosted_org_units: Array.isArray(payload?.org_units) ? payload.org_units : [],
    hosted_capabilities: Array.isArray(payload?.capabilities) ? payload.capabilities.map(String) : [],
    hosted_policy_version: payload?.policy_version == null ? null : Number(payload.policy_version),
    hosted_user: payload.user,
    principal_id: userId,
    principal_source: "tandem-hosted",
    principal_scope: payloadDeployment,
  };
}

export function assertSameHostedIdentity(previous, next) {
  if (previous.principal_id !== next.principal_id || previous.principal_scope !== next.principal_scope) {
    throw new Error("Hosted session refresh changed user or deployment; sign in again.");
  }
}

// These local/sidecar handlers currently use deployment-wide files, settings or
// administrative credentials, rather than per-user runtime authorization.
// Until they gain a governed user path, a hosted member must not reach them.
const ADMIN_ROUTES = [
  "/api/control-panel/config", "/api/system/search-settings", "/api/system/scheduler-settings",
  "/api/knowledgebase", "/api/aca", "/api/swarm", "/api/orchestrator",
  "/api/files", "/api/workspace/files",
];

export function hostedPanelRouteAllowed(session, pathname) {
  if (session?.hosted !== true) return true;
  // Match the existing router's prefix checks, including unusual suffixes.
  const administrative = ADMIN_ROUTES.some((path) => pathname.startsWith(path));
  if (!administrative) return true;
  const role = String(session.hosted_role || "").toLowerCase();
  const capabilities = new Set((session.hosted_capabilities || []).map((value) => String(value).toLowerCase()));
  return role === "owner" || role === "admin" || capabilities.has("hosted.owner") || capabilities.has("hosted.admin");
}
