import { APP_NAV_ROUTES, type RouteId } from "./routes";

export const NAV_VISIBILITY_STORAGE_KEY = "tcp.nav.visibility.v1";

export const ACA_CORE_NAV_ROUTE_IDS = new Set<RouteId>([
  "dashboard",
  "chat",
  "planner",
  "workflows",
  "marketplace",
  "coding",
  "settings",
]);

export type NavigationVisibility = Record<RouteId, boolean>;

export function getDefaultNavigationVisibility(acaMode: boolean): NavigationVisibility {
  const standaloneHiddenRoutes = new Set<RouteId>(["coding", "memory", "feed"]);

  return Object.fromEntries(
    APP_NAV_ROUTES.map(([routeId]) => [
      routeId,
      acaMode
        ? ACA_CORE_NAV_ROUTE_IDS.has(routeId as RouteId)
        : !standaloneHiddenRoutes.has(routeId as RouteId),
    ])
  ) as NavigationVisibility;
}

export function normalizeNavigationVisibility(
  raw: unknown,
  acaMode: boolean
): NavigationVisibility {
  const defaults = getDefaultNavigationVisibility(acaMode);
  if (!raw || typeof raw !== "object") return defaults;
  const candidate = raw as Record<string, unknown>;
  const next = { ...defaults };
  for (const [routeId] of APP_NAV_ROUTES) {
    if (typeof candidate[routeId] === "boolean") {
      next[routeId as RouteId] = candidate[routeId] as boolean;
    }
  }
  if (acaMode) {
    for (const routeId of ACA_CORE_NAV_ROUTE_IDS) {
      next[routeId] = true;
    }
  }
  return next;
}

export function loadNavigationVisibility(acaMode: boolean): NavigationVisibility {
  try {
    const raw = localStorage.getItem(NAV_VISIBILITY_STORAGE_KEY);
    if (!raw) return getDefaultNavigationVisibility(acaMode);
    return normalizeNavigationVisibility(JSON.parse(raw), acaMode);
  } catch {
    return getDefaultNavigationVisibility(acaMode);
  }
}

export function saveNavigationVisibility(visibility: NavigationVisibility) {
  try {
    localStorage.setItem(NAV_VISIBILITY_STORAGE_KEY, JSON.stringify(visibility));
  } catch {
    // Ignore storage failures.
  }
}

export function visibleNavigationRoutes(visibility: NavigationVisibility) {
  return APP_NAV_ROUTES.filter(([routeId]) => visibility[routeId as RouteId] !== false);
}
