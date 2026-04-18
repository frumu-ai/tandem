import { useQuery } from "@tanstack/react-query";
import { api, isTransientEngineError } from "../../lib/api";

export function useSystemHealth(enabled = true) {
  return useQuery({
    queryKey: ["system", "health"],
    queryFn: () => api("/api/system/health"),
    enabled,
    refetchInterval: enabled ? 15000 : false,
    retry: (failureCount, error) =>
      isTransientEngineError(error) ? failureCount < 6 : failureCount < 2,
    retryDelay: (attempt) => Math.min(1000 * 2 ** attempt, 10000),
  });
}

export function useSwarmStatus(enabled = true) {
  return useQuery({
    queryKey: ["swarm", "status"],
    queryFn: () => api("/api/swarm/status"),
    enabled,
    refetchInterval: enabled ? 5000 : false,
    retry: (failureCount, error) =>
      isTransientEngineError(error) ? failureCount < 6 : failureCount < 2,
    retryDelay: (attempt) => Math.min(1000 * 2 ** attempt, 10000),
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
  control_panel_mode?: "aca" | "standalone" | "auto";
  control_panel_mode_source?: "env" | "config" | "detected" | string;
  control_panel_mode_reason?: string;
  control_panel_config_path?: string;
  control_panel_config_ready?: boolean;
  control_panel_config_missing?: string[];
  control_panel_compact_nav?: boolean;
  hosted_managed?: boolean;
  hosted_provider?: string;
  hosted_deployment_id?: string;
  hosted_deployment_slug?: string;
  hosted_hostname?: string;
  hosted_public_url?: string;
  hosted_control_plane_url?: string;
  hosted_release_version?: string;
  hosted_release_channel?: string;
  hosted_update_policy?: string;
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
