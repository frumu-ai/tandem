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

export interface Capabilities {
  aca_integration: boolean;
  aca_reason: string;
  coding_workflows: boolean;
  missions: boolean;
  agent_teams: boolean;
  coder: boolean;
  engine_healthy: boolean;
  cached_at_ms: number;
}

export function useCapabilities(enabled = true) {
  return useQuery({
    queryKey: ["system", "capabilities"],
    queryFn: () => api("/api/capabilities") as Promise<Capabilities>,
    enabled,
    refetchInterval: enabled ? 60000 : false,
    staleTime: 30000,
  });
}
