// Browser requests never provide runtime identity. Only the server-held session
// may attach a context assertion after these headers have been removed.
const IDENTITY_HEADERS = new Set([
  "x-tandem-context-assertion",
  "x-tandem-context-jws",
  "x-tandem-tenant-context-jws",
  "x-tandem-org-id",
  "x-tenant-org-id",
  "x-tandem-workspace-id",
  "x-tenant-workspace-id",
  "x-tandem-deployment-id",
  "x-tandem-actor-id",
  "x-user-id",
  "x-tandem-principal-id",
  "x-tandem-roles",
  "x-tandem-capabilities",
  "x-tandem-agent-id",
  "x-tandem-agent-ancestor-ids",
  "x-tandem-request-source",
]);

export function isEngineIdentityHeader(name) {
  return IDENTITY_HEADERS.has(String(name).toLowerCase());
}

export function sessionEngineHeaders(session, extraHeaders = {}) {
  const headers = new Headers(extraHeaders);
  for (const name of [...headers.keys()]) {
    if (isEngineIdentityHeader(name)) headers.delete(name);
  }
  headers.set("authorization", `Bearer ${session.token}`);
  headers.set("x-tandem-token", session.token);
  if (session.context_assertion) {
    headers.set("x-tandem-context-assertion", String(session.context_assertion));
  }
  return headers;
}
