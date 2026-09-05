// Hosted credentials may leave the panel only for its operator-configured
// control plane. Endpoint overrides are paths on that same origin, not new
// credential recipients. Plain HTTP is supported only for literal loopback
// addresses used by local adapters and integration tests.
export function hostedAuthEndpoint(auth, operation) {
  if (!auth?.managed || !["exchange", "refresh"].includes(operation)) {
    throw new Error("Hosted panel authentication is not configured.");
  }
  let controlPlane;
  let endpoint;
  try {
    controlPlane = new URL(auth.controlPlaneUrl);
    endpoint = new URL(operation === "exchange" ? auth.panelExchangeUrl : auth.panelRefreshUrl);
  } catch {
    throw new Error("Hosted authentication requires absolute control-plane URLs.");
  }
  const loopback = ["127.0.0.1", "[::1]"].includes(controlPlane.hostname);
  if (controlPlane.protocol !== "https:" && !(controlPlane.protocol === "http:" && loopback)) {
    throw new Error("Hosted authentication requires HTTPS outside loopback.");
  }
  if (controlPlane.username || controlPlane.password || controlPlane.search || controlPlane.hash ||
      endpoint.username || endpoint.password || endpoint.search || endpoint.hash) {
    throw new Error("Hosted authentication URLs cannot contain credentials, queries or fragments.");
  }
  const basePath = controlPlane.pathname.replace(/\/+$/, "");
  if (endpoint.origin !== controlPlane.origin || !endpoint.pathname.startsWith(`${basePath}/`)) {
    throw new Error("Hosted authentication endpoints must belong to the configured control plane.");
  }
  return endpoint;
}
