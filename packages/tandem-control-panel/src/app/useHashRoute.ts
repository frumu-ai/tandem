import { useCallback, useEffect, useRef, useState } from "react";
import { ensureRouteId, routeFromHash, setHashRoute, type RouteId } from "./routes";

type HashRouteOptions = {
  canNavigate?: (next: RouteId, current: RouteId) => boolean;
};

export function useHashRoute(options: HashRouteOptions = {}) {
  const [route, setRouteState] = useState<RouteId>(() => routeFromHash());
  const routeRef = useRef(route);
  const canNavigateRef = useRef(options.canNavigate);

  useEffect(() => {
    routeRef.current = route;
  }, [route]);

  useEffect(() => {
    canNavigateRef.current = options.canNavigate;
  }, [options.canNavigate]);

  useEffect(() => {
    const canonical = routeFromHash(routeRef.current);
    if (routeFromHash(canonical) !== canonical) {
      setHashRoute(canonical);
    }
  }, []);

  const commitRoute = useCallback((next: RouteId, source: "navigate" | "hashchange") => {
    const current = routeRef.current;
    if (next === current) {
      if (routeFromHash(current) !== current) {
        setHashRoute(current);
        return true;
      }
      setRouteState(current);
      return true;
    }
    const allowed = canNavigateRef.current ? canNavigateRef.current(next, current) : true;
    if (!allowed) {
      if (source === "hashchange" && window.location.hash !== `#/${current}`) {
        window.history.replaceState(
          null,
          "",
          `${window.location.pathname}${window.location.search}#/${current}`
        );
      }
      return false;
    }
    if (window.location.hash !== `#/${next}`) {
      setHashRoute(next);
      return true;
    }
    setRouteState(next);
    return true;
  }, []);

  useEffect(() => {
    const onHashChange = () => {
      const next = routeFromHash();
      commitRoute(next, "hashchange");
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [commitRoute]);

  const navigate = useCallback(
    (next: string) => {
      const safe = ensureRouteId(next);
      commitRoute(safe, "navigate");
    },
    [commitRoute]
  );

  return { route, navigate };
}
