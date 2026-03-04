import { NAV_ROUTES, ROUTES } from "./store.js";

export const APP_ROUTES = ROUTES;
export const APP_NAV_ROUTES = NAV_ROUTES;

export type RouteId =
  | "dashboard"
  | "chat"
  | "automations"
  | "agents"
  | "channels"
  | "mcp"
  | "packs"
  | "swarm"
  | "files"
  | "memory"
  | "teams"
  | "feed"
  | "settings";

// Legacy routes that should redirect to the new automations page
const LEGACY_TO_AUTOMATIONS = new Set(["agents", "packs", "teams"]);

const routeSet = new Set(APP_ROUTES.map(([id]) => id));

export function ensureRouteId(route: string, fallback: RouteId = "dashboard"): RouteId {
  if (LEGACY_TO_AUTOMATIONS.has(route)) return "automations";
  return routeSet.has(route) ? (route as RouteId) : fallback;
}

export function routeFromHash(defaultRoute: RouteId = "dashboard"): RouteId {
  const raw = (window.location.hash || `#/${defaultRoute}`).replace(/^#\//, "");
  return ensureRouteId(raw.split("?")[0].split("/")[0].trim(), defaultRoute);
}

export function setHashRoute(route: RouteId) {
  window.location.hash = `#/${route}`;
}
