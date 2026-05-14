import { useMemo } from "react";

export function useOverlapHistoryEntries(automationsV2: any[], toArray: any) {
  return useMemo(() => {
    const rows: Array<Record<string, any>> = [];
    for (const automation of automationsV2) {
      const automationId = String(
        automation?.automation_id || automation?.automationId || automation?.id || ""
      ).trim();
      const automationName = String(automation?.name || "").trim();
      const planPackage = automation?.metadata?.plan_package || automation?.metadata?.planPackage;
      const overlapLog = toArray(planPackage?.overlap_policy, "overlap_log");
      const sourcePlanId = String(
        planPackage?.plan_id || planPackage?.planId || automationId || ""
      ).trim();
      const sourcePlanRevision = Number(
        planPackage?.plan_revision || planPackage?.planRevision || 0
      );
      const sourceLifecycleState = String(
        planPackage?.lifecycle_state || planPackage?.lifecycleState || automation?.status || ""
      ).trim();
      for (const entry of overlapLog) {
        rows.push({
          rowKey: [
            automationId || sourcePlanId || "automation",
            String(entry?.matched_plan_id || entry?.matchedPlanId || ""),
            String(entry?.matched_plan_revision || entry?.matchedPlanRevision || ""),
            String(entry?.decision || ""),
            String(entry?.decided_at || entry?.decidedAt || ""),
          ].join(":"),
          sourceLabel: automationName || automationId || sourcePlanId || "workflow plan",
          sourceAutomationId: automationId,
          sourcePlanId,
          sourcePlanRevision: Number.isFinite(sourcePlanRevision) ? sourcePlanRevision : 0,
          sourceLifecycleState,
          matchedPlanId: String(entry?.matched_plan_id || entry?.matchedPlanId || "").trim(),
          matchedPlanRevision: Number(
            entry?.matched_plan_revision || entry?.matchedPlanRevision || 0
          ),
          matchLayer: String(entry?.match_layer || entry?.matchLayer || "").trim(),
          similarityScore: entry?.similarity_score ?? entry?.similarityScore ?? null,
          decision: String(entry?.decision || "").trim(),
          decidedBy: String(entry?.decided_by || entry?.decidedBy || "").trim(),
          decidedAt: String(entry?.decided_at || entry?.decidedAt || "").trim(),
        });
      }
    }
    return rows.sort((left, right) => {
      const leftAt = Number(Date.parse(String(left.decidedAt || "")));
      const rightAt = Number(Date.parse(String(right.decidedAt || "")));
      if (Number.isFinite(leftAt) && Number.isFinite(rightAt) && leftAt !== rightAt) {
        return rightAt - leftAt;
      }
      return String(left.sourcePlanId || left.sourceAutomationId || left.rowKey).localeCompare(
        String(right.sourcePlanId || right.sourceAutomationId || right.rowKey)
      );
    });
  }, [automationsV2, toArray]);
}
