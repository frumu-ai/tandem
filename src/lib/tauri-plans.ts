// Tauri bindings for plan management commands
import { invoke } from "@tauri-apps/api/core";

export interface PlanInfo {
  sessionName: string;
  fileName: string;
  fullPath: string;
  lastModified: number;
}

/**
 * List all plans in the workspace, grouped by session
 */
export async function listPlans(): Promise<PlanInfo[]> {
  return invoke<PlanInfo[]>("list_plans");
}

/**
 * Read the content of a plan file
 */
export async function readPlanContent(planPath: string): Promise<string> {
  return invoke<string>("read_plan_content", { planPath });
}
