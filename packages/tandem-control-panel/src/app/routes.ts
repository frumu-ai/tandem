import { NAV_ROUTES, ROUTES } from "./store.js";

export const APP_ROUTES = ROUTES;
export const APP_NAV_ROUTES = NAV_ROUTES;

export type RouteId =
  | "dashboard"
  | "chat"
  | "planner"
  | "workflows"
  | "studio"
  | "automations"
  | "coding"
  | "orchestrator"
  | "agents"
  | "channels"
  | "mcp"
  | "bug-monitor"
  | "packs"
  | "files"
  | "memory"
  | "teams"
  | "feed"
  | "settings"
  | "packs-detail"
  | "teams-detail";

// Legacy routes that should redirect to modern surfaces
const LEGACY_ROUTE_REDIRECTS = new Map<string, RouteId>([
  ["packs", "automations"],
  ["teams", "automations"],
  ["swarm", "orchestrator"],
  ["failure-reporter", "bug-monitor"],
]);

const routeSet = new Set(APP_ROUTES.map(([id]) => id));

export function ensureRouteId(route: string, fallback: RouteId = "dashboard"): RouteId {
  const redirected = LEGACY_ROUTE_REDIRECTS.get(String(route || "").trim());
  if (redirected) return redirected;
  return routeSet.has(route) ? (route as RouteId) : fallback;
}

export function routeFromHash(defaultRoute: RouteId = "dashboard"): RouteId {
  const raw = (window.location.hash || `#/${defaultRoute}`).replace(/^#\//, "");
  return ensureRouteId(raw.split("?")[0].split("/")[0].trim(), defaultRoute);
}

export function setHashRoute(route: RouteId) {
  window.location.hash = `#/${route}`;
}
