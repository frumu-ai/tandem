import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";

export function useSystemHealth(enabled = true) {
  return useQuery({
    queryKey: ["system", "health"],
    queryFn: () => api("/api/system/health"),
    enabled,
    refetchInterval: enabled ? 15000 : false,
  });
}

export function useSwarmStatus(enabled = true) {
  return useQuery({
    queryKey: ["swarm", "status"],
    queryFn: () => api("/api/swarm/status"),
    enabled,
    refetchInterval: enabled ? 5000 : false,
  });
}
