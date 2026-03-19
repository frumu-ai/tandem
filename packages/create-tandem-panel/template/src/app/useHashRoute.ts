import { useCallback, useEffect, useState } from "react";
import { ensureRouteId, routeFromHash, setHashRoute, type RouteId } from "./routes";

export function useHashRoute() {
  const [route, setRouteState] = useState<RouteId>(() => routeFromHash());

  useEffect(() => {
    const onHash = () => setRouteState(routeFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const navigate = useCallback((next: string) => {
    const safe = ensureRouteId(next);
    if (window.location.hash !== `#/${safe}`) {
      setHashRoute(safe);
      return;
    }
    setRouteState(safe);
  }, []);

  return { route, navigate };
}
