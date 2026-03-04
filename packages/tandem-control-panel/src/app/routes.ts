import { NAV_ROUTES, ROUTES } from "./store.js";

export const APP_ROUTES = ROUTES;
export const APP_NAV_ROUTES = NAV_ROUTES;

export type RouteId =
  | "dashboard"
  | "chat"
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

const routeSet = new Set(APP_ROUTES.map(([id]) => id));

export function ensureRouteId(route: string, fallback: RouteId = "dashboard"): RouteId {
  return routeSet.has(route) ? (route as RouteId) : fallback;
}

export function routeFromHash(defaultRoute: RouteId = "dashboard"): RouteId {
  const raw = (window.location.hash || `#/${defaultRoute}`).replace(/^#\//, "");
  return ensureRouteId(raw.split("?")[0].split("/")[0].trim(), defaultRoute);
}

export function setHashRoute(route: RouteId) {
  window.location.hash = `#/${route}`;
}
