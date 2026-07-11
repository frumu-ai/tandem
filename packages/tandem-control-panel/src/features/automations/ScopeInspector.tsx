import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { formatJson } from "../../pages/ui";
import { LazyJson } from "./LazyJson";
import { api } from "../../lib/api";
import { ConnectorSuggestionPanel } from "./ConnectorSuggestionPanel";
import { PlanReplayComparePanel } from "./PlanReplayComparePanel";
import { SharedWorkflowContextPanel } from "./SharedWorkflowContextPanel";
import { ScopeInspectorReplayCheck } from "./ScopeInspectorReplayCheck";
import {
  approvalModeLabel,
  artifactVisibilityLabel,
  booleanBadge,
  downloadJsonFile,
  formatPercentage,
  historyVisibilityLabel,
  kv,
  listPaths,
  packSuggestionReason,
  parseCommaSeparatedProjectKeys,
  planApprovalMatrixRows,
  prettyEnumLabel,
  safeString,
  statusBadge,
  successCriteriaStatus,
  toArray,
  tokenizeSuggestionTerms,
} from "./scopeInspectorPrimitives";
import type {
  ArtifactVisibilityFilter,
  HistoryVisibilityFilter,
  RoutineGraphDependency,
  RoutineGraphLayer,
  RoutineGraphNode,
  ScopeInspectorProps,
  ScopeView,
} from "./scopeInspectorPrimitives";
import { Icon } from "../../ui/Icon";
import { SearchInput } from "../../ui/index.tsx";

export function ScopeInspector({
  planPackage,
  planPackageBundle,
  planPackageReplay,
  validationReport,
  runtimeContext,
  approvedPlanMaterialization,
  overlapHistoryEntries,
  title = "Scope inspector",
  onOpenPromptEditor,
  onOpenModelRoutingEditor,
  onOpenConnectorBindingsEditor,
  onReplaceSharedContextPack,
}: ScopeInspectorProps) {
  const [view, setView] = useState<ScopeView>("all");
  const [historyVisibilityFilter, setHistoryVisibilityFilter] =
    useState<HistoryVisibilityFilter>("all");
  const [artifactVisibilityFilter, setArtifactVisibilityFilter] =
    useState<ArtifactVisibilityFilter>("all");
  const [overlapHistorySearch, setOverlapHistorySearch] = useState("");
  const [bundleShareStatus, setBundleShareStatus] = useState("");
  const [contextPackStatus, setContextPackStatus] = useState("");
  const [selectedContextPackId, setSelectedContextPackId] = useState("");
  const [sharedContextAllowlistInput, setSharedContextAllowlistInput] = useState("");
  const routines = useMemo(() => toArray(planPackage?.routine_graph), [planPackage]);
  const workspaceRoot = useMemo(
    () =>
      safeString(
        planPackage?.workspace_root ||
          planPackage?.workspaceRoot ||
          approvedPlanMaterialization?.workspace_root ||
          approvedPlanMaterialization?.workspaceRoot
      ),
    [approvedPlanMaterialization, planPackage]
  );
  const projectKey = useMemo(
    () =>
      safeString(
        planPackage?.project_key ||
          planPackage?.projectKey ||
          approvedPlanMaterialization?.project_key ||
          approvedPlanMaterialization?.projectKey
      ),
    [approvedPlanMaterialization, planPackage]
  );
  const contextPacksQuery = useQuery({
    queryKey: ["context-packs", "scope-inspector", workspaceRoot, projectKey],
    enabled: !!workspaceRoot,
    queryFn: () =>
      api(
        `/api/engine/context/packs?workspace_root=${encodeURIComponent(workspaceRoot)}${
          projectKey ? `&project_key=${encodeURIComponent(projectKey)}` : ""
        }`
      ).catch(() => ({ context_packs: [] })),
  });
  const connectorBindingResolution = useMemo(
    () => planPackage?.connector_binding_resolution || null,
    [planPackage]
  );
  const modelRoutingResolution = useMemo(
    () => planPackage?.model_routing_resolution || null,
    [planPackage]
  );
  const routineGraph = useMemo(() => {
    const routineEntries = routines.map((routine: any, index: number) => [
      safeString(routine?.routine_id || routine?.id || `routine-${index + 1}`),
      routine,
    ]) as Array<[string, any]>;
    const routineMap = new Map<string, any>(routineEntries);
    const downstreamMap = new Map<string, Set<string>>();

    routineEntries.forEach(([routineId, routine]) => {
      toArray(routine?.dependencies).forEach((dependency: any) => {
        const upstreamId = safeString(dependency?.routine_id);
        if (!upstreamId) return;
        if (!downstreamMap.has(upstreamId)) {
          downstreamMap.set(upstreamId, new Set());
        }
        downstreamMap.get(upstreamId)?.add(routineId);
      });
    });

    const layerCache = new Map<string, number>();
    const visiting = new Set<string>();

    function resolveLayer(routineId: string): number {
      if (layerCache.has(routineId)) return layerCache.get(routineId) || 0;
      const routine = routineMap.get(routineId);
      if (!routine) {
        layerCache.set(routineId, 0);
        return 0;
      }
      if (visiting.has(routineId)) return 0;
      visiting.add(routineId);
      let layer = 0;
      for (const dependency of toArray(routine?.dependencies)) {
        const upstreamId = safeString(dependency?.routine_id);
        if (!upstreamId || !routineMap.has(upstreamId)) continue;
        layer = Math.max(layer, resolveLayer(upstreamId) + 1);
      }
      visiting.delete(routineId);
      layerCache.set(routineId, layer);
      return layer;
    }

    const nodes: RoutineGraphNode[] = routineEntries
      .map(([routineId, routine], index) => {
        const dependencies: RoutineGraphDependency[] = toArray(routine?.dependencies)
          .map((dependency: any) => {
            const upstreamId = safeString(dependency?.routine_id);
            if (!upstreamId) return null;
            return {
              routineId: upstreamId,
              dependencyType: prettyEnumLabel(dependency?.dependency_type || dependency?.type),
              mode: prettyEnumLabel(dependency?.mode),
              resolved: routineMap.has(upstreamId),
            };
          })
          .filter(Boolean);

        return {
          routineId,
          routine,
          index,
          layer: resolveLayer(routineId),
          stepCount: toArray(routine?.steps).length,
          downstreamCount: downstreamMap.get(routineId)?.size || 0,
          dependencies,
        };
      })
      .sort(
        (left, right) => left.layer - right.layer || left.routineId.localeCompare(right.routineId)
      );

    const layers: RoutineGraphLayer[] = Array.from(
      nodes.reduce((acc, node) => {
        if (!acc.has(node.layer)) acc.set(node.layer, []);
        acc.get(node.layer)?.push(node);
        return acc;
      }, new Map<number, RoutineGraphNode[]>())
    )
      .map(([layer, items]) => ({ layer, items }))
      .sort((left, right) => left.layer - right.layer);

    return { nodes, layers };
  }, [routines]);
  const successCriteriaReport = useMemo(
    () =>
      planPackage?.success_criteria_report ||
      planPackage?.success_criteria_evaluation ||
      planPackage?.validation_state?.success_criteria_evaluation ||
      validationReport?.success_criteria_report ||
      validationReport?.success_criteria_evaluation ||
      null,
    [planPackage, validationReport]
  );
  const successCriteriaEntries = useMemo(
    () => toArray(successCriteriaReport?.entries),
    [successCriteriaReport]
  );
  const planSuccessCriteria = useMemo(
    () =>
      successCriteriaReport?.plan ||
      successCriteriaReport?.plan_summary ||
      successCriteriaReport?.plan_evaluation ||
      successCriteriaReport?.plan_coverage ||
      successCriteriaEntries.find((entry: any) => safeString(entry?.subject) === "plan") ||
      null,
    [successCriteriaEntries, successCriteriaReport]
  );
  const routineSuccessCriteria = useMemo(
    () =>
      toArray(
        successCriteriaReport?.routines ||
          successCriteriaReport?.routine_evaluations ||
          successCriteriaReport?.routine_coverage
      ).length
        ? toArray(
            successCriteriaReport?.routines ||
              successCriteriaReport?.routine_evaluations ||
              successCriteriaReport?.routine_coverage
          )
        : successCriteriaEntries.filter((entry: any) => safeString(entry?.subject) === "routine"),
    [successCriteriaEntries, successCriteriaReport]
  );
  const stepSuccessCriteria = useMemo(
    () =>
      toArray(
        successCriteriaReport?.steps ||
          successCriteriaReport?.step_evaluations ||
          successCriteriaReport?.step_coverage
      ).length
        ? toArray(
            successCriteriaReport?.steps ||
              successCriteriaReport?.step_evaluations ||
              successCriteriaReport?.step_coverage
          )
        : successCriteriaEntries.filter((entry: any) => safeString(entry?.subject) === "step"),
    [successCriteriaEntries, successCriteriaReport]
  );
  const credentialEnvelopes = useMemo(
    () => toArray(planPackage?.credential_envelopes),
    [planPackage]
  );
  const contextObjects = useMemo(() => toArray(planPackage?.context_objects), [planPackage]);
  const runtimePartitions = useMemo(() => toArray(runtimeContext?.routines), [runtimeContext]);
  const approvedRoutines = useMemo(
    () => toArray(approvedPlanMaterialization?.routines),
    [approvedPlanMaterialization]
  );
  const approvalPolicy = useMemo(() => planPackage?.approval_policy || null, [planPackage]);
  const replayIssues = useMemo(() => toArray(planPackageReplay?.issues), [planPackageReplay]);
  const replayRecommendation = useMemo(() => {
    if (!planPackageReplay) return null;
    const scopePreserved = planPackageReplay.scope_metadata_preserved === true;
    const handoffPreserved = planPackageReplay.handoff_rules_preserved === true;
    const credentialPreserved = planPackageReplay.credential_isolation_preserved === true;
    const hasBlockingIssue = replayIssues.some((issue: any) => {
      if (issue?.blocking) return true;
      const severity = safeString(issue?.severity).toLowerCase();
      return severity === "error";
    });

    if (planPackageReplay.compatible && scopePreserved && handoffPreserved && credentialPreserved) {
      return {
        label: "reuse",
        tone: "success",
        reason: "Replay is compatible and scope, handoff, and credential preservation are intact.",
      };
    }

    if (!scopePreserved || !handoffPreserved || !credentialPreserved || hasBlockingIssue) {
      return {
        label: "fork",
        tone: "warning",
        reason: hasBlockingIssue
          ? "Replay issues include blocking or error entries."
          : "Scope, handoff, or credential preservation was not maintained.",
      };
    }

    return {
      label: "refresh",
      tone: "info",
      reason: "Drift detected without an isolation break; refresh the plan while keeping lineage.",
    };
  }, [planPackageReplay, replayIssues]);
  const [showReplayIssues, setShowReplayIssues] = useState(false);
  const overlapHistoryRows = useMemo(() => {
    const rows = toArray(overlapHistoryEntries).length
      ? toArray(overlapHistoryEntries)
      : toArray(planPackage?.overlap_policy?.overlap_log).map((entry: any, index: number) => ({
          sourceLabel: safeString(
            planPackage?.name || planPackage?.title || title || "Current plan"
          ),
          sourceAutomationId: safeString(planPackage?.plan_id || planPackage?.planId || ""),
          sourcePlanId: safeString(planPackage?.plan_id || planPackage?.planId || ""),
          sourcePlanRevision: Number(planPackage?.plan_revision || planPackage?.planRevision || 0),
          sourceLifecycleState: safeString(
            planPackage?.lifecycle_state || planPackage?.lifecycleState || "unknown"
          ),
          matchedPlanId: safeString(entry?.matched_plan_id || entry?.matchedPlanId || ""),
          matchedPlanRevision: Number(
            entry?.matched_plan_revision || entry?.matchedPlanRevision || 0
          ),
          matchLayer: safeString(entry?.match_layer || entry?.matchLayer || ""),
          similarityScore: entry?.similarity_score ?? entry?.similarityScore ?? null,
          decision: safeString(entry?.decision || ""),
          decidedBy: safeString(entry?.decided_by || entry?.decidedBy || ""),
          decidedAt: safeString(entry?.decided_at || entry?.decidedAt || ""),
          rowKey: `${safeString(
            planPackage?.plan_id || planPackage?.planId || title || "plan"
          )}-${index}`,
        }));
    return rows
      .map((entry: any, index: number) => ({
        sourceLabel: safeString(
          entry?.sourceLabel ||
            entry?.automation_name ||
            entry?.automationName ||
            entry?.sourceAutomationName ||
            entry?.automationId ||
            entry?.sourceAutomationId ||
            title
        ),
        sourceAutomationId: safeString(
          entry?.sourceAutomationId || entry?.automation_id || entry?.automationId || ""
        ),
        sourcePlanId: safeString(entry?.sourcePlanId || entry?.plan_id || entry?.planId || ""),
        sourcePlanRevision: Number(
          entry?.sourcePlanRevision ?? entry?.plan_revision ?? entry?.planRevision ?? 0
        ),
        sourceLifecycleState: safeString(
          entry?.sourceLifecycleState ||
            entry?.lifecycle_state ||
            entry?.lifecycleState ||
            "unknown"
        ),
        matchedPlanId: safeString(
          entry?.matchedPlanId || entry?.matched_plan_id || entry?.matchedPlanID || ""
        ),
        matchedPlanRevision: Number(
          entry?.matchedPlanRevision ?? entry?.matched_plan_revision ?? 0
        ),
        matchLayer: safeString(entry?.matchLayer || entry?.match_layer || ""),
        similarityScore: entry?.similarityScore ?? entry?.similarity_score ?? null,
        decision: safeString(entry?.decision || ""),
        decidedBy: safeString(entry?.decidedBy || entry?.decided_by || ""),
        decidedAt: safeString(entry?.decidedAt || entry?.decided_at || ""),
        rowKey:
          safeString(entry?.rowKey || entry?.key || entry?.id) ||
          `${safeString(entry?.matchedPlanId || entry?.matched_plan_id || "overlap")}-${index}`,
      }))
      .sort((left: any, right: any) => {
        const leftAt = Number(Date.parse(left.decidedAt || ""));
        const rightAt = Number(Date.parse(right.decidedAt || ""));
        if (Number.isFinite(leftAt) && Number.isFinite(rightAt) && leftAt !== rightAt) {
          return rightAt - leftAt;
        }
        return String(left.sourcePlanId || left.sourceAutomationId || left.rowKey).localeCompare(
          String(right.sourcePlanId || right.sourceAutomationId || right.rowKey)
        );
      });
  }, [overlapHistoryEntries, planPackage, title]);
  const filteredOverlapHistoryRows = useMemo(() => {
    const query = safeString(overlapHistorySearch).toLowerCase();
    if (!query) return overlapHistoryRows;
    return overlapHistoryRows.filter((entry: any) =>
      [
        entry.sourceLabel,
        entry.sourceAutomationId,
        entry.sourcePlanId,
        entry.sourcePlanRevision,
        entry.sourceLifecycleState,
        entry.matchedPlanId,
        entry.matchedPlanRevision,
        entry.matchLayer,
        entry.similarityScore,
        entry.decision,
        entry.decidedBy,
        entry.decidedAt,
      ]
        .map((value) => safeString(value).toLowerCase())
        .join(" ")
        .includes(query)
    );
  }, [overlapHistoryRows, overlapHistorySearch]);
  const filteredHistoryRoutines = useMemo(() => {
    if (historyVisibilityFilter === "all" && artifactVisibilityFilter === "all") return routines;
    return routines.filter((routine: any) => {
      const visibility = safeString(routine?.audit_scope?.run_history_visibility).toLowerCase();
      const artifactVisibility = safeString(
        routine?.audit_scope?.final_artifact_visibility
      ).toLowerCase();
      const matchesHistory =
        historyVisibilityFilter === "all" || visibility === historyVisibilityFilter;
      const matchesArtifact =
        artifactVisibilityFilter === "all" || artifactVisibility === artifactVisibilityFilter;
      return matchesHistory && matchesArtifact;
    });
  }, [artifactVisibilityFilter, historyVisibilityFilter, routines]);
  const planValidationState =
    validationReport?.validation_state ||
    validationReport?.validationState ||
    planPackage?.validation_state ||
    planPackage?.validationState ||
    validationReport ||
    null;
  const validationSummaryRows = useMemo(
    () => [
      ["ready for apply", validationReport?.ready_for_apply],
      ["ready for activation", validationReport?.ready_for_activation],
      ["approvals complete", planValidationState?.approvals_complete],
      ["activation ready", planValidationState?.compartmentalized_activation_ready],
      ["blockers", validationReport?.blocker_count],
      ["warnings", validationReport?.warning_count],
    ],
    [planValidationState, validationReport]
  );
  const successCriteriaMissingCount = useMemo(() => {
    const entries = [planSuccessCriteria, ...routineSuccessCriteria, ...stepSuccessCriteria].filter(
      Boolean
    );
    return entries.reduce((sum: number, entry: any) => {
      const missing = toArray(
        entry?.missing_required_artifacts ||
          entry?.missing_artifacts ||
          entry?.missing_required ||
          entry?.missing
      );
      return sum + missing.length;
    }, 0);
  }, [planSuccessCriteria, routineSuccessCriteria, stepSuccessCriteria]);
  const analyticsSummary = useMemo(() => {
    const successCriteriaDefined = successCriteriaReport?.defined_count ?? null;
    const successCriteriaTotal = successCriteriaReport?.total_subjects ?? null;
    const overlapLog = toArray(planPackage?.overlap_policy?.overlap_log);
    const forkCount = overlapLog.filter(
      (entry: any) => safeString(entry?.decision).toLowerCase() === "fork"
    ).length;
    return {
      successCoverage:
        successCriteriaDefined !== null && successCriteriaTotal
          ? {
              label: formatPercentage(successCriteriaDefined, successCriteriaTotal),
              detail: `${successCriteriaDefined}/${successCriteriaTotal} subjects defined`,
            }
          : null,
      approvalReadiness:
        planValidationState?.approvals_complete === true ||
        planValidationState?.approvals_complete === false
          ? {
              label: planValidationState.approvals_complete ? "100%" : "0%",
              detail: planValidationState.approvals_complete
                ? "approvals complete"
                : "approvals still pending",
            }
          : null,
      overlapForkRate: overlapLog.length
        ? {
            label: formatPercentage(forkCount, overlapLog.length),
            detail: `${forkCount}/${overlapLog.length} overlap decisions forked`,
          }
        : null,
    };
  }, [planPackage, planValidationState, successCriteriaReport]);
  const contextPacks = useMemo(() => {
    const nowMs = Date.now();
    return toArray((contextPacksQuery.data as any)?.context_packs)
      .map((pack: any) => ({
        packId: safeString(pack?.pack_id),
        title: safeString(pack?.title || pack?.summary || pack?.pack_id || "shared context"),
        state: safeString(pack?.state || "published"),
        sourcePlanId: safeString(pack?.source_plan_id || pack?.manifest?.plan_package?.plan_id),
        projectKey: safeString(pack?.project_key || ""),
        visibilityScope: safeString(pack?.visibility_scope || "same_project"),
        allowedProjectKeys: toArray(pack?.allowed_project_keys)
          .map((entry: any) => safeString(entry))
          .filter(Boolean),
        bindings: toArray(pack?.bindings),
        freshnessWindowHours: pack?.freshness_window_hours,
        updatedAtMs: Number(pack?.updated_at_ms || pack?.published_at_ms || 0),
        isStale:
          Number(pack?.freshness_window_hours || 0) > 0 &&
          Number(pack?.updated_at_ms || pack?.published_at_ms || 0) > 0 &&
          nowMs - Number(pack?.updated_at_ms || pack?.published_at_ms || 0) >
            Number(pack?.freshness_window_hours || 0) * 60 * 60 * 1000,
        raw: pack,
      }))
      .sort((left: any, right: any) => right.updatedAtMs - left.updatedAtMs);
  }, [contextPacksQuery.data]);
  useEffect(() => {
    if (!contextPacks.length) {
      if (selectedContextPackId) setSelectedContextPackId("");
      return;
    }
    if (
      !selectedContextPackId ||
      !contextPacks.some((pack: any) => pack.packId === selectedContextPackId)
    ) {
      setSelectedContextPackId(contextPacks[0].packId);
    }
  }, [contextPacks, selectedContextPackId]);
  const selectedContextPack = useMemo(
    () => contextPacks.find((pack: any) => pack.packId === selectedContextPackId) || null,
    [contextPacks, selectedContextPackId]
  );
  const sharedContextBindingRows = useMemo(() => {
    const packById = new Map(contextPacks.map((pack: any) => [pack.packId, pack]));
    const candidateSources = [
      planPackage?.shared_context_bindings,
      planPackage?.sharedContextBindings,
      planPackage?.shared_context_pack_ids,
      planPackage?.sharedContextPackIds,
      approvedPlanMaterialization?.shared_context_bindings,
      approvedPlanMaterialization?.sharedContextBindings,
      approvedPlanMaterialization?.shared_context_pack_ids,
      approvedPlanMaterialization?.sharedContextPackIds,
    ];
    const ids: Array<{
      packId: string;
      required: boolean;
      alias: string;
    }> = [];
    for (const source of candidateSources) {
      if (!Array.isArray(source)) continue;
      for (const entry of source) {
        if (typeof entry === "string") {
          const packId = safeString(entry);
          if (!packId) continue;
          if (!ids.some((row) => row.packId === packId)) {
            ids.push({ packId, required: true, alias: "" });
          }
          continue;
        }
        if (!entry || typeof entry !== "object") continue;
        const packId = safeString(
          entry.pack_id || entry.packId || entry.context_pack_id || entry.contextPackId || entry.id
        );
        if (!packId || ids.some((row) => row.packId === packId)) continue;
        ids.push({
          packId,
          required: entry.required !== false,
          alias: safeString(entry.alias || entry.name || entry.label || ""),
        });
      }
    }
    return ids.map((row) => ({
      ...row,
      pack: packById.get(row.packId) || null,
    }));
  }, [approvedPlanMaterialization, contextPacks, planPackage]);
  const suggestedContextPacks = useMemo(() => {
    const currentSourcePlanId = safeString(
      planPackage?.source_plan_id ||
        planPackage?.sourcePlanId ||
        approvedPlanMaterialization?.source_plan_id ||
        approvedPlanMaterialization?.sourcePlanId ||
        planPackage?.plan_id ||
        planPackage?.planId ||
        approvedPlanMaterialization?.plan_id ||
        approvedPlanMaterialization?.planId
    );
    const currentTitleTerms = [
      ...tokenizeSuggestionTerms(planPackage?.title),
      ...tokenizeSuggestionTerms(planPackage?.name),
      ...tokenizeSuggestionTerms(approvedPlanMaterialization?.title),
    ];
    const boundIds = new Set(sharedContextBindingRows.map((entry: any) => entry.packId));
    return contextPacks
      .filter((pack: any) => !boundIds.has(pack.packId))
      .filter((pack: any) => safeString(pack.state) === "published")
      .filter((pack: any) => !pack.isStale)
      .map((pack: any) => {
        let score = 0;
        if (currentSourcePlanId && pack.sourcePlanId === currentSourcePlanId) score += 6;
        if (pack.updatedAtMs) score += 1;
        const packTerms = new Set([
          ...tokenizeSuggestionTerms(pack.title),
          ...tokenizeSuggestionTerms(pack.raw?.summary),
        ]);
        const overlap = currentTitleTerms.filter((term) => packTerms.has(term));
        score += Math.min(3, overlap.length);
        return {
          ...pack,
          score,
          reason: packSuggestionReason(pack, currentSourcePlanId, currentTitleTerms),
        };
      })
      .sort(
        (left: any, right: any) => right.score - left.score || right.updatedAtMs - left.updatedAtMs
      )
      .slice(0, 3);
  }, [approvedPlanMaterialization, contextPacks, planPackage, sharedContextBindingRows]);
  const supersededContextBindingRows = useMemo(
    () =>
      sharedContextBindingRows.filter(
        (entry: any) =>
          entry.pack?.state === "superseded" && safeString(entry.pack?.raw?.superseded_by_pack_id)
      ),
    [sharedContextBindingRows]
  );

  async function publishCurrentContextPack() {
    if (!workspaceRoot) {
      setContextPackStatus("Workspace root is not available for this plan.");
      return;
    }
    setContextPackStatus("Publishing shared workflow context...");
    try {
      const contextObjectRefs = toArray(planPackage?.context_objects)
        .map((entry: any) => safeString(entry?.context_object_id || entry?.contextObjectId))
        .filter(Boolean);
      const artifactRefs = toArray(
        approvedPlanMaterialization?.artifact_refs ||
          approvedPlanMaterialization?.artifactRefs ||
          planPackage?.artifact_refs ||
          planPackage?.artifactRefs
      )
        .map((entry: any) => safeString(entry))
        .filter(Boolean);
      const governedMemoryRefs = toArray(
        planPackage?.governed_memory_refs || planPackage?.governedMemoryRefs
      )
        .map((entry: any) => safeString(entry))
        .filter(Boolean);
      const payload = {
        title: safeString(
          planPackage?.title ||
            planPackage?.name ||
            approvedPlanMaterialization?.title ||
            approvedPlanMaterialization?.name ||
            planPackage?.plan_id ||
            "Shared workflow context"
        ),
        summary: safeString(
          planPackage?.summary || approvedPlanMaterialization?.summary || "Shared workflow context"
        ),
        workspace_root: workspaceRoot,
        ...(projectKey ? { project_key: projectKey } : {}),
        source_plan_id: safeString(
          planPackage?.plan_id || planPackage?.planId || approvedPlanMaterialization?.plan_id || ""
        ),
        source_context_run_id: safeString(
          approvedPlanMaterialization?.context_run_id ||
            approvedPlanMaterialization?.contextRunId ||
            planPackage?.context_run_id ||
            planPackage?.contextRunId ||
            ""
        ),
        plan_package: planPackage,
        approved_plan_materialization: approvedPlanMaterialization,
        runtime_context: runtimeContext,
        context_object_refs: contextObjectRefs,
        artifact_refs: artifactRefs,
        governed_memory_refs: governedMemoryRefs,
        allowed_project_keys: parseCommaSeparatedProjectKeys(sharedContextAllowlistInput),
      };
      const response = await api("/api/engine/context/packs", {
        method: "POST",
        body: JSON.stringify(payload),
      });
      setContextPackStatus(
        `Published ${safeString(response?.context_pack?.pack_id || payload.title)}.`
      );
    } catch (error) {
      setContextPackStatus(error instanceof Error ? error.message : "Publish failed.");
    }
  }

  if (!planPackage) {
    return (
      <div className="grid gap-2 rounded-xl border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
        <div className="font-medium text-slate-100">{title}</div>
        <div className="tcp-subtle">
          No stored plan package is available yet. Apply a workflow plan first to inspect scope,
          credential envelopes, and handoff contracts here.
        </div>
      </div>
    );
  }

  return (
    <div className="grid gap-3 rounded-xl border border-slate-700/60 bg-slate-950/20 p-3 text-xs text-slate-300">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div>
          <div className="font-medium text-slate-100">{title}</div>
          <div className="tcp-subtle">
            plan: {safeString(planPackage?.plan_id) || "n/a"} · revision:{" "}
            {safeString(planPackage?.plan_revision) || "n/a"} · routines: {routines.length}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-1">
          {(
            [
              "all",
              "scope",
              "graph",
              "compare",
              "credentials",
              "handoffs",
              "runtime",
              "audit",
            ] as ScopeView[]
          ).map((candidate) => (
            <button
              key={candidate}
              type="button"
              aria-pressed={view === candidate}
              className={`tcp-btn h-7 px-2 tcp-text-caption ${
                view === candidate ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""
              }`}
              onClick={() => setView(candidate)}
            >
              {candidate === "all" ? "All" : candidate.charAt(0).toUpperCase() + candidate.slice(1)}
            </button>
          ))}
          {planPackageBundle ? (
            <>
              <button
                type="button"
                className="tcp-btn h-7 px-2 tcp-text-caption"
                onClick={() =>
                  downloadJsonFile(
                    `${safeString(planPackage?.plan_id) || "plan"}-bundle.json`,
                    planPackageBundle
                  )
                }
              >
                <Icon name="download" />
                Export bundle
              </button>
              <button
                type="button"
                className="tcp-btn h-7 px-2 tcp-text-caption"
                onClick={async () => {
                  try {
                    await navigator.clipboard.writeText(formatJson(planPackageBundle));
                    setBundleShareStatus("Copied bundle JSON.");
                  } catch (error) {
                    setBundleShareStatus(error instanceof Error ? error.message : "Copy failed.");
                  }
                }}
              >
                <Icon name="copy" />
                Copy bundle
              </button>
              {bundleShareStatus ? (
                <span className="tcp-subtle tcp-text-caption">{bundleShareStatus}</span>
              ) : null}
            </>
          ) : null}
        </div>
      </div>
      {planPackageReplay ? (
        <ScopeInspectorReplayCheck
          planPackageReplay={planPackageReplay}
          replayIssues={replayIssues}
          replayRecommendation={replayRecommendation}
          showReplayIssues={showReplayIssues}
          onToggleShowReplayIssues={() => setShowReplayIssues((current) => !current)}
        />
      ) : null}
      {(view === "all" || view === "compare") && planPackageReplay ? (
        <PlanReplayComparePanel planPackageReplay={planPackageReplay} />
      ) : null}
      {(view === "all" || view === "scope") && routines.length ? (
        <div className="grid gap-2">
          <div className="font-medium text-slate-200">Routine scope preview</div>
          <div className="grid gap-2">
            {routines.map((routine: any, index: number) => (
              <div
                key={String(routine?.routine_id || routine?.id || index)}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">
                    {safeString(routine?.routine_id || routine?.id || `routine-${index + 1}`)}
                  </div>
                  <span className="tcp-badge-info">
                    {safeString(routine?.kind || routine?.semantic_kind || "routine")}
                  </span>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("mission context", routine?.data_scope?.mission_context_scope)}
                  {kv("cross-routine visibility", routine?.data_scope?.cross_routine_visibility)}
                  {kv(
                    "inter-routine model",
                    planPackage?.inter_routine_policy?.communication_model
                  )}
                  {kv(
                    "artifact handoff validation",
                    planPackage?.inter_routine_policy?.artifact_handoff_validation
                  )}
                </div>
                <div className="mt-3 grid gap-2 sm:grid-cols-2">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      readable paths
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {listPaths(routine?.data_scope?.readable_paths)}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      writable paths
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {listPaths(routine?.data_scope?.writable_paths)}
                    </div>
                  </div>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      denied paths
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {listPaths(routine?.data_scope?.denied_paths)}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      audit visibility
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {safeString(routine?.audit_scope?.run_history_visibility) || "n/a"} ·{" "}
                      {safeString(routine?.audit_scope?.intermediate_artifact_visibility) || "n/a"}{" "}
                      · {safeString(routine?.audit_scope?.final_artifact_visibility) || "n/a"}
                    </div>
                  </div>
                </div>
                {Array.isArray(routine?.steps) && routine.steps.length ? (
                  <div className="mt-2 tcp-text-caption text-slate-400">
                    steps: {routine.steps.length} · dependencies:{" "}
                    {routine.steps
                      .map((step: any) => safeString(step?.step_id || step?.id))
                      .filter(Boolean)
                      .join(", ")}
                  </div>
                ) : null}
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "graph") && routines.length ? (
        <div className="grid gap-2">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="font-medium text-slate-200">Routine dependency graph</div>
            <div className="tcp-subtle tcp-text-caption">
              Read-only graph derived from `routine_graph.dependencies`
            </div>
          </div>
          {modelRoutingResolution ? (
            <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
              <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                Model routing summary
              </div>
              <div className="mt-2 grid gap-2 sm:grid-cols-3">
                {kv("tier assigned", modelRoutingResolution?.tier_assigned_count)}
                {kv("provider unresolved", modelRoutingResolution?.provider_unresolved_count)}
                {kv("steps", toArray(modelRoutingResolution?.entries).length)}
              </div>
            </div>
          ) : null}
          <div className="overflow-x-auto pb-1">
            <div className="flex min-w-max items-start gap-3">
              {routineGraph.layers.map(({ layer, items }) => (
                <div
                  key={`layer-${layer}`}
                  className="grid min-w-[20rem] max-w-[24rem] flex-1 gap-2"
                >
                  <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                    Layer {layer} · {items.length} routine{items.length === 1 ? "" : "s"}
                  </div>
                  <div className="grid gap-2">
                    {items.map((node) => {
                      const routineStepIds = toArray(node.routine?.step_ids).length
                        ? toArray(node.routine?.step_ids)
                            .map((stepId: any) => safeString(stepId))
                            .filter(Boolean)
                        : toArray(node.routine?.steps)
                            .map((step: any) =>
                              safeString(step?.step_id || step?.stepId || step?.id)
                            )
                            .filter(Boolean);

                      return (
                        <div
                          key={node.routineId}
                          className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
                        >
                          <div className="flex flex-wrap items-center justify-between gap-2">
                            <div className="font-medium text-slate-100">{node.routineId}</div>
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="tcp-badge-info">
                                {prettyEnumLabel(node.routine?.semantic_kind || "routine")}
                              </span>
                              <span className="tcp-badge-success">
                                {node.stepCount} step{node.stepCount === 1 ? "" : "s"}
                              </span>
                              <span className="tcp-badge-info">
                                {node.downstreamCount} downstream
                              </span>
                              {onOpenPromptEditor ? (
                                <button
                                  type="button"
                                  className="tcp-btn h-7 px-2 tcp-text-caption"
                                  onClick={onOpenPromptEditor}
                                >
                                  <Icon name="arrow-right" />
                                  Open in editor
                                </button>
                              ) : null}
                            </div>
                          </div>
                          {routineStepIds.length ? (
                            <div className="mt-2">
                              <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                                step ids
                              </div>
                              <div className="mt-2 flex flex-wrap gap-2">
                                {routineStepIds.map((stepId) => (
                                  <span
                                    key={`${node.routineId}-${stepId}`}
                                    className="rounded-full border border-slate-700/80 bg-slate-900/80 px-2 py-1 tcp-text-caption text-slate-100"
                                  >
                                    {stepId}
                                  </span>
                                ))}
                              </div>
                            </div>
                          ) : null}
                          <div className="mt-2 grid gap-2 sm:grid-cols-2">
                            {kv(
                              "dependency resolution",
                              node.routine?.dependency_resolution?.strategy
                                ? [
                                    prettyEnumLabel(node.routine?.dependency_resolution?.strategy),
                                    prettyEnumLabel(
                                      node.routine?.dependency_resolution?.partial_failure_mode
                                    ),
                                    prettyEnumLabel(
                                      node.routine?.dependency_resolution?.reentry_point
                                    ),
                                  ]
                                    .filter(Boolean)
                                    .join(" · ")
                                : "n/a"
                            )}
                            {kv(
                              "trigger",
                              prettyEnumLabel(node.routine?.trigger?.trigger_type || "n/a")
                            )}
                          </div>
                          <div className="mt-2">
                            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                              Upstream dependencies
                            </div>
                            {node.dependencies.length ? (
                              <div className="mt-2 flex flex-wrap gap-2">
                                {node.dependencies.map((dependency: any) => (
                                  <span
                                    key={`${node.routineId}-${dependency.routineId}`}
                                    className={
                                      dependency.resolved
                                        ? "rounded-full border border-slate-700/80 bg-slate-900/80 px-2 py-1 tcp-text-caption text-slate-100"
                                        : "rounded-full border border-amber-500/40 bg-amber-500/10 px-2 py-1 tcp-text-caption text-amber-50"
                                    }
                                  >
                                    <span className="font-medium">{dependency.routineId}</span>
                                    <span className="tcp-subtle"> · </span>
                                    <span>{dependency.dependencyType}</span>
                                    <span className="tcp-subtle"> · </span>
                                    <span>{dependency.mode}</span>
                                  </span>
                                ))}
                              </div>
                            ) : (
                              <div className="mt-1 text-sm text-slate-400">
                                No upstream dependencies.
                              </div>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "credentials") && credentialEnvelopes.length ? (
        <div className="grid gap-2">
          <div className="font-medium text-slate-200">Credential envelopes</div>
          <div className="grid gap-2">
            {credentialEnvelopes.map((envelope: any, index: number) => (
              <div
                key={String(envelope?.routine_id || index)}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">
                    {safeString(envelope?.routine_id || `routine-${index + 1}`)}
                  </div>
                  <span className="tcp-badge-info">
                    {safeString(envelope?.issuing_authority || "engine")}
                  </span>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div>
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      entitled connectors
                    </div>
                    <LazyJson
                      value={envelope?.entitled_connectors || []}
                      className="mt-1"
                      preClassName="tcp-code mt-1 max-h-28 overflow-auto tcp-text-caption"
                    />
                  </div>
                  <div>
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      denied connectors
                    </div>
                    <LazyJson
                      value={envelope?.denied_connectors || []}
                      className="mt-1"
                      preClassName="tcp-code mt-1 max-h-28 overflow-auto tcp-text-caption"
                    />
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "credentials") &&
      (connectorBindingResolution || toArray(planPackage?.connector_intents).length) ? (
        <div className="grid gap-2">
          <ConnectorSuggestionPanel planPackage={planPackage} />
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="font-medium text-slate-200">Connector binding resolution</div>
            {onOpenConnectorBindingsEditor ? (
              <button
                type="button"
                className="tcp-btn h-7 px-2 tcp-text-caption"
                onClick={onOpenConnectorBindingsEditor}
              >
                <Icon name="settings-2" />
                Edit connectors
              </button>
            ) : null}
          </div>
          <div className="tcp-subtle tcp-text-caption">
            Read-only summary. Edit bindings in the workflow connector bindings block.
          </div>
          <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
            <div className="grid gap-2 sm:grid-cols-3">
              {kv("mapped", connectorBindingResolution?.mapped_count)}
              {kv("unresolved required", connectorBindingResolution?.unresolved_required_count)}
              {kv("unresolved optional", connectorBindingResolution?.unresolved_optional_count)}
            </div>
          </div>
          <div className="grid gap-2">
            {toArray(connectorBindingResolution?.entries).map((entry: any, index: number) => (
              <div
                key={String(entry?.capability || index)}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">
                    {safeString(entry?.capability || `binding-${index + 1}`)}
                  </div>
                  <span
                    className={
                      entry?.resolved
                        ? "tcp-badge-success"
                        : entry?.required
                          ? "tcp-badge-warning"
                          : "tcp-badge-info"
                    }
                  >
                    {safeString(entry?.status || "unresolved")}
                  </span>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("required", entry?.required)}
                  {kv("degraded mode", entry?.degraded_mode_allowed)}
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("binding type", entry?.binding_type)}
                  {kv("binding id", entry?.binding_id)}
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("why", entry?.why)}
                  {kv("allowlist pattern", entry?.allowlist_pattern)}
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "runtime") && modelRoutingResolution ? (
        <div className="grid gap-2">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="font-medium text-slate-200">Model routing</div>
            {onOpenModelRoutingEditor ? (
              <button
                type="button"
                className="tcp-btn h-7 px-2 tcp-text-caption"
                onClick={onOpenModelRoutingEditor}
              >
                <Icon name="settings-2" />
                Edit routing
              </button>
            ) : null}
          </div>
          <div className="tcp-subtle tcp-text-caption">
            Read-only summary. Edit the workflow default provider/model in the workflow model
            selection block, and per-step overrides in the prompt editor cards.
          </div>
          <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
            <div className="grid gap-2 sm:grid-cols-3">
              {kv("tier assigned", modelRoutingResolution?.tier_assigned_count)}
              {kv("provider unresolved", modelRoutingResolution?.provider_unresolved_count)}
              {kv("steps", toArray(modelRoutingResolution?.entries).length)}
            </div>
          </div>
          <div className="grid gap-2">
            {toArray(modelRoutingResolution?.entries).map((entry: any, index: number) => (
              <div
                key={String(entry?.step_id || index)}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">
                    {safeString(entry?.step_id || `step-${index + 1}`)}
                  </div>
                  <span className={entry?.resolved ? "tcp-badge-success" : "tcp-badge-warning"}>
                    {safeString(entry?.status || "unresolved")}
                  </span>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("tier", entry?.tier)}
                  {kv("provider id", entry?.provider_id)}
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("model id", entry?.model_id)}
                  {kv("reason", entry?.reason)}
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "audit") && successCriteriaReport ? (
        <div className="grid gap-2">
          <div className="font-medium text-slate-200">Success criteria coverage</div>
          <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
            <div className="grid gap-2 sm:grid-cols-4">
              {kv("plan status", successCriteriaStatus(planSuccessCriteria))}
              {kv("routines reported", routineSuccessCriteria.length)}
              {kv("steps reported", stepSuccessCriteria.length)}
              {kv("missing artifacts", successCriteriaMissingCount)}
            </div>
          </div>
          <div className="grid gap-2">
            <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="font-medium text-slate-100">Plan success criteria</div>
                {statusBadge(successCriteriaStatus(planSuccessCriteria))}
              </div>
              {planSuccessCriteria ? (
                <>
                  <div className="mt-2 grid gap-2 sm:grid-cols-3">
                    {kv(
                      "required artifacts",
                      toArray(
                        planSuccessCriteria?.required_artifacts ||
                          planSuccessCriteria?.required_artifact_ids ||
                          planSuccessCriteria?.required
                      ).length
                    )}
                    {kv(
                      "missing artifacts",
                      toArray(
                        planSuccessCriteria?.missing_required_artifacts ||
                          planSuccessCriteria?.missing_artifacts ||
                          planSuccessCriteria?.missing_required ||
                          planSuccessCriteria?.missing
                      ).length
                    )}
                    {kv(
                      "freshness window",
                      planSuccessCriteria?.freshness_window_hours ??
                        planSuccessCriteria?.freshness_window
                    )}
                  </div>
                  <div className="mt-2 grid gap-2 sm:grid-cols-2">
                    <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                      <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                        required artifacts
                      </div>
                      <div className="mt-1 break-words text-slate-100">
                        {listPaths(
                          planSuccessCriteria?.required_artifacts ||
                            planSuccessCriteria?.required_artifact_ids ||
                            planSuccessCriteria?.required
                        )}
                      </div>
                    </div>
                    <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                      <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                        missing artifacts
                      </div>
                      <div className="mt-1 break-words text-slate-100">
                        {listPaths(
                          planSuccessCriteria?.missing_required_artifacts ||
                            planSuccessCriteria?.missing_artifacts ||
                            planSuccessCriteria?.missing_required ||
                            planSuccessCriteria?.missing
                        )}
                      </div>
                    </div>
                  </div>
                  {safeString(
                    planSuccessCriteria?.minimum_viable_completion ||
                      planSuccessCriteria?.minimum_output
                  ) ? (
                    <div className="mt-2 text-slate-100">
                      minimum:{" "}
                      {safeString(
                        planSuccessCriteria?.minimum_viable_completion ||
                          planSuccessCriteria?.minimum_output
                      )}
                    </div>
                  ) : null}
                </>
              ) : (
                <div className="mt-2 text-slate-400">No plan-level criteria reported.</div>
              )}
            </div>
          </div>
          <div className="grid gap-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">Routine criteria</div>
            {routineSuccessCriteria.length ? (
              routineSuccessCriteria.map((entry: any, index: number) => {
                const routineId = safeString(
                  entry?.routine_id || entry?.id || `routine-${index + 1}`
                );
                return (
                  <div
                    key={routineId}
                    className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="font-medium text-slate-100">{routineId}</div>
                      {statusBadge(successCriteriaStatus(entry))}
                    </div>
                    <div className="mt-2 grid gap-2 sm:grid-cols-3">
                      {kv(
                        "required artifacts",
                        toArray(
                          entry?.required_artifacts ||
                            entry?.required_artifact_ids ||
                            entry?.required
                        ).length
                      )}
                      {kv(
                        "missing artifacts",
                        toArray(
                          entry?.missing_required_artifacts ||
                            entry?.missing_artifacts ||
                            entry?.missing_required ||
                            entry?.missing
                        ).length
                      )}
                      {kv(
                        "freshness window",
                        entry?.freshness_window_hours ?? entry?.freshness_window
                      )}
                    </div>
                    <div className="mt-2 grid gap-2 sm:grid-cols-2">
                      <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                        <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                          required artifacts
                        </div>
                        <div className="mt-1 break-words text-slate-100">
                          {listPaths(
                            entry?.required_artifacts ||
                              entry?.required_artifact_ids ||
                              entry?.required
                          )}
                        </div>
                      </div>
                      <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                        <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                          missing artifacts
                        </div>
                        <div className="mt-1 break-words text-slate-100">
                          {listPaths(
                            entry?.missing_required_artifacts ||
                              entry?.missing_artifacts ||
                              entry?.missing_required ||
                              entry?.missing
                          )}
                        </div>
                      </div>
                    </div>
                    {safeString(
                      entry?.minimum_viable_completion || entry?.minimum_output || entry?.summary
                    ) ? (
                      <div className="mt-2 text-slate-100">
                        minimum:{" "}
                        {safeString(
                          entry?.minimum_viable_completion ||
                            entry?.minimum_output ||
                            entry?.summary
                        )}
                      </div>
                    ) : null}
                  </div>
                );
              })
            ) : (
              <div className="tcp-subtle text-xs">No routine-level criteria reported.</div>
            )}
          </div>
          <div className="grid gap-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">Step criteria</div>
            {stepSuccessCriteria.length ? (
              stepSuccessCriteria.map((entry: any, index: number) => {
                const stepId = safeString(entry?.step_id || entry?.id || `step-${index + 1}`);
                return (
                  <div
                    key={stepId}
                    className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="font-medium text-slate-100">{stepId}</div>
                      {statusBadge(successCriteriaStatus(entry))}
                    </div>
                    <div className="mt-2 grid gap-2 sm:grid-cols-3">
                      {kv(
                        "required artifacts",
                        toArray(
                          entry?.required_artifacts ||
                            entry?.required_artifact_ids ||
                            entry?.required
                        ).length
                      )}
                      {kv(
                        "missing artifacts",
                        toArray(
                          entry?.missing_required_artifacts ||
                            entry?.missing_artifacts ||
                            entry?.missing_required ||
                            entry?.missing
                        ).length
                      )}
                      {kv(
                        "freshness window",
                        entry?.freshness_window_hours ?? entry?.freshness_window
                      )}
                    </div>
                    <div className="mt-2 grid gap-2 sm:grid-cols-2">
                      <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                        <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                          required artifacts
                        </div>
                        <div className="mt-1 break-words text-slate-100">
                          {listPaths(
                            entry?.required_artifacts ||
                              entry?.required_artifact_ids ||
                              entry?.required
                          )}
                        </div>
                      </div>
                      <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                        <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                          missing artifacts
                        </div>
                        <div className="mt-1 break-words text-slate-100">
                          {listPaths(
                            entry?.missing_required_artifacts ||
                              entry?.missing_artifacts ||
                              entry?.missing_required ||
                              entry?.missing
                          )}
                        </div>
                      </div>
                    </div>
                    {safeString(
                      entry?.minimum_viable_completion || entry?.minimum_output || entry?.summary
                    ) ? (
                      <div className="mt-2 text-slate-100">
                        minimum:{" "}
                        {safeString(
                          entry?.minimum_viable_completion ||
                            entry?.minimum_output ||
                            entry?.summary
                        )}
                      </div>
                    ) : null}
                  </div>
                );
              })
            ) : (
              <div className="tcp-subtle text-xs">No step-level criteria reported.</div>
            )}
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "handoffs") && contextObjects.length ? (
        <div className="grid gap-2">
          <div className="font-medium text-slate-200">Handoff contracts</div>
          <div className="grid gap-2">
            {contextObjects.map((contextObject: any, index: number) => (
              <div
                key={String(contextObject?.context_object_id || index)}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="font-medium text-slate-100">
                    {safeString(contextObject?.context_object_id || `context-${index + 1}`)}
                  </div>
                  <span className="tcp-badge-info">
                    {safeString(contextObject?.kind || contextObject?.scope || "context")}
                  </span>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  {kv("owner routine", contextObject?.owner_routine_id)}
                  {kv("producer step", contextObject?.producer_step_id)}
                  {kv("scope", contextObject?.scope)}
                  {kv("validation", contextObject?.validation_status)}
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      declared consumers
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {listPaths(contextObject?.declared_consumers)}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      data scope refs
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {listPaths(contextObject?.data_scope_refs)}
                    </div>
                  </div>
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      artifact ref
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {safeString(contextObject?.artifact_ref) || "n/a"}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      freshness window
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {safeString(contextObject?.freshness_window_hours) || "n/a"} hours
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "runtime") && runtimePartitions.length ? (
        <div className="grid gap-2">
          <div className="font-medium text-slate-200">Runtime context partitions</div>
          <div className="grid gap-2">
            {runtimePartitions.map((partition: any, index: number) => (
              <div
                key={String(partition?.routine_id || index)}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="font-medium text-slate-100">
                  {safeString(partition?.routine_id || `routine-${index + 1}`)}
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div>
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      visible context objects
                    </div>
                    <LazyJson
                      value={partition?.visible_context_objects || []}
                      className="mt-1"
                      preClassName="tcp-code mt-1 max-h-28 overflow-auto tcp-text-caption"
                    />
                  </div>
                  <div>
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      step context bindings
                    </div>
                    <LazyJson
                      value={partition?.step_context_bindings || []}
                      className="mt-1"
                      preClassName="tcp-code mt-1 max-h-28 overflow-auto tcp-text-caption"
                    />
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {(view === "all" || view === "runtime") && approvedPlanMaterialization ? (
        <div className="grid gap-2">
          <div className="font-medium text-slate-200">Approved plan materialization</div>
          <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
            <div className="grid gap-2 sm:grid-cols-4">
              {kv("plan id", approvedPlanMaterialization?.plan_id)}
              {kv("revision", approvedPlanMaterialization?.plan_revision)}
              {kv("routines", approvedPlanMaterialization?.routine_count)}
              {kv("context objects", approvedPlanMaterialization?.context_object_count)}
            </div>
            {kv("lifecycle state", approvedPlanMaterialization?.lifecycle_state)}
            {runtimePartitions.length ? (
              <div className="mt-2 tcp-text-caption text-slate-400">
                The runtime context below is derived from the approved materialization.
              </div>
            ) : null}
          </div>
          <div className="grid gap-2">
            {approvedRoutines.map((routine: any, index: number) => (
              <div
                key={String(routine?.routine_id || index)}
                className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
              >
                <div className="font-medium text-slate-100">
                  {safeString(routine?.routine_id || `routine-${index + 1}`)}
                </div>
                <div className="mt-2 grid gap-2 sm:grid-cols-2">
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">step ids</div>
                    <div className="mt-1 break-words text-slate-100">
                      {listPaths(routine?.step_ids)}
                    </div>
                  </div>
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      visible context objects
                    </div>
                    <div className="mt-1 break-words text-slate-100">
                      {listPaths(routine?.visible_context_object_ids)}
                    </div>
                  </div>
                </div>
                <div className="mt-2">
                  <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                    step context bindings
                  </div>
                  <LazyJson
                    value={routine?.step_context_bindings || []}
                    className="mt-1"
                    preClassName="tcp-code mt-1 max-h-28 overflow-auto tcp-text-caption"
                  />
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {workspaceRoot ? (
        <SharedWorkflowContextPanel
          workspaceRoot={workspaceRoot}
          projectKey={projectKey}
          sharedContextBindingRows={sharedContextBindingRows}
          supersededContextBindingRows={supersededContextBindingRows}
          contextPacks={contextPacks}
          selectedContextPack={selectedContextPack}
          suggestedContextPacks={suggestedContextPacks}
          sharedContextAllowlistInput={sharedContextAllowlistInput}
          contextPackStatus={contextPackStatus}
          onAllowlistChange={setSharedContextAllowlistInput}
          onSelectContextPack={setSelectedContextPackId}
          onPublishCurrentContextPack={() => {
            void publishCurrentContextPack();
          }}
          onContextPackStatusChange={setContextPackStatus}
          onReplaceSharedContextPack={onReplaceSharedContextPack}
        />
      ) : null}

      {(view === "all" || view === "audit") && planValidationState ? (
        <div className="grid gap-2" role="region" aria-label="Validation and history visibility">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="font-medium text-slate-200">Audit / provenance</div>
            <div className="flex flex-wrap items-center gap-1">
              {(
                ["all", "routine_only", "plan_owner", "named_roles"] as HistoryVisibilityFilter[]
              ).map((candidate) => (
                <button
                  key={candidate}
                  type="button"
                  aria-pressed={historyVisibilityFilter === candidate}
                  className={`tcp-btn h-7 px-2 tcp-text-caption ${
                    historyVisibilityFilter === candidate
                      ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                      : ""
                  }`}
                  onClick={() => setHistoryVisibilityFilter(candidate)}
                >
                  {candidate === "all"
                    ? "All"
                    : historyVisibilityLabel(candidate).replace(/\s+/g, " ")}
                </button>
              ))}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
              Artifact visibility
            </div>
            <div className="flex flex-wrap items-center gap-1">
              {(
                [
                  "all",
                  "routine_only",
                  "declared_consumers",
                  "plan_owner",
                  "workspace",
                ] as ArtifactVisibilityFilter[]
              ).map((candidate) => (
                <button
                  key={candidate}
                  type="button"
                  aria-pressed={artifactVisibilityFilter === candidate}
                  className={`tcp-btn h-7 px-2 tcp-text-caption ${
                    artifactVisibilityFilter === candidate
                      ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                      : ""
                  }`}
                  onClick={() => setArtifactVisibilityFilter(candidate)}
                >
                  {candidate === "all" ? "All" : artifactVisibilityLabel(candidate)}
                </button>
              ))}
            </div>
          </div>
          <div className="tcp-subtle tcp-text-caption">
            Validation and history visibility, {filteredHistoryRoutines.length} routine
            {filteredHistoryRoutines.length === 1 ? "" : "s"} visible.
          </div>
          <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
            {kv("data scopes valid", planValidationState?.data_scopes_valid)}
            {kv("audit scopes valid", planValidationState?.audit_scopes_valid)}
            {kv("credential envelopes valid", planValidationState?.credential_envelopes_valid)}
            {kv("context objects valid", planValidationState?.context_objects_valid)}
          </div>
          <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
            {kv(
              "connector mappings",
              planValidationState?.required_connectors_mapped === true
                ? "mapped"
                : planValidationState?.required_connectors_mapped === false
                  ? "unmapped"
                  : "n/a"
            )}
            {kv(
              "directories writable",
              planValidationState?.directories_writable === true
                ? "yes"
                : planValidationState?.directories_writable === false
                  ? "no"
                  : "n/a"
            )}
            {kv(
              "schedules valid",
              planValidationState?.schedules_valid === true
                ? "yes"
                : planValidationState?.schedules_valid === false
                  ? "no"
                  : "n/a"
            )}
            {kv(
              "models resolved",
              planValidationState?.models_resolved === true
                ? "yes"
                : planValidationState?.models_resolved === false
                  ? "no"
                  : "n/a"
            )}
          </div>
          <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
            {kv(
              "dependencies resolvable",
              planValidationState?.dependencies_resolvable === true
                ? "yes"
                : planValidationState?.dependencies_resolvable === false
                  ? "no"
                  : "n/a"
            )}
            {kv(
              "approvals complete",
              planValidationState?.approvals_complete === true
                ? "yes"
                : planValidationState?.approvals_complete === false
                  ? "no"
                  : "n/a"
            )}
            {kv(
              "activation ready",
              planValidationState?.compartmentalized_activation_ready === true
                ? "yes"
                : planValidationState?.compartmentalized_activation_ready === false
                  ? "no"
                  : "n/a"
            )}
            {kv(
              "degraded modes acknowledged",
              planValidationState?.degraded_modes_acknowledged === true
                ? "yes"
                : planValidationState?.degraded_modes_acknowledged === false
                  ? "no"
                  : "n/a"
            )}
          </div>
          <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="font-medium text-slate-100">Validation summary</div>
              <span className="tcp-badge-info">apply / activation</span>
            </div>
            <div className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
              {validationSummaryRows.map(([label, value]) => (
                <div
                  key={label}
                  className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                >
                  <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">{label}</div>
                  <div className="mt-1">
                    {typeof value === "boolean" ? (
                      booleanBadge(value)
                    ) : (
                      <span className="tcp-badge-info">
                        {value === null || value === undefined ? "n/a" : String(value)}
                      </span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </div>
          {analyticsSummary ? (
            <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="font-medium text-slate-100">Analytics summary</div>
                <span className="tcp-badge-info">derived from current snapshot</span>
              </div>
              <div className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
                {analyticsSummary.successCoverage ? (
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      success coverage
                    </div>
                    <div className="mt-1 text-slate-100">
                      {analyticsSummary.successCoverage.label}
                    </div>
                    <div className="tcp-subtle tcp-text-caption">
                      {analyticsSummary.successCoverage.detail}
                    </div>
                  </div>
                ) : null}
                {analyticsSummary.approvalReadiness ? (
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      approval readiness
                    </div>
                    <div className="mt-1 text-slate-100">
                      {analyticsSummary.approvalReadiness.label}
                    </div>
                    <div className="tcp-subtle tcp-text-caption">
                      {analyticsSummary.approvalReadiness.detail}
                    </div>
                  </div>
                ) : null}
                {analyticsSummary.overlapForkRate ? (
                  <div className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2">
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      overlap fork rate
                    </div>
                    <div className="mt-1 text-slate-100">
                      {analyticsSummary.overlapForkRate.label}
                    </div>
                    <div className="tcp-subtle tcp-text-caption">
                      {analyticsSummary.overlapForkRate.detail}
                    </div>
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}
          {overlapHistoryRows.length ? (
            <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="font-medium text-slate-100">Searchable overlap history</div>
                <span className="tcp-badge-info">
                  {filteredOverlapHistoryRows.length}/{overlapHistoryRows.length} decisions
                </span>
              </div>
              <div className="mt-2 grid gap-2">
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Search overlap history</label>
                  <SearchInput
                    aria-label="Search overlap history"
                    className="tcp-input"
                    value={overlapHistorySearch}
                    onInput={(event) =>
                      setOverlapHistorySearch((event.target as HTMLInputElement).value)
                    }
                    placeholder="Search automation id, plan id, revision, decision, decided by, or source"
                  />
                </div>
                <div className="text-xs text-slate-500">
                  Searchable across loaded workflow plans. Use it to find prior reuse, merge, fork,
                  and new decisions by plan or source.
                </div>
                {filteredOverlapHistoryRows.length ? (
                  filteredOverlapHistoryRows.map((entry: any, index: number) => (
                    <div
                      key={`${safeString(entry?.rowKey || entry?.matchedPlanId || entry?.sourcePlanId || index)}-${index}`}
                      className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="font-medium text-slate-100">
                          {safeString(
                            entry?.sourcePlanId ||
                              entry?.sourceAutomationId ||
                              entry?.matchedPlanId ||
                              `overlap-${index + 1}`
                          )}
                        </div>
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="tcp-badge-info">
                            {safeString(entry?.decision || "unknown")}
                          </span>
                          {safeString(entry?.sourceLifecycleState) ? (
                            <span className="tcp-badge-info">
                              {safeString(entry?.sourceLifecycleState)}
                            </span>
                          ) : null}
                        </div>
                      </div>
                      <div className="mt-2 text-xs text-slate-400">
                        source{" "}
                        {safeString(entry?.sourceLabel || entry?.sourceAutomationId || "n/a")}
                      </div>
                      <div className="mt-2 grid gap-2 sm:grid-cols-4">
                        {kv("plan revision", entry?.sourcePlanRevision)}
                        {kv("matched plan", entry?.matchedPlanId)}
                        {kv("matched revision", entry?.matchedPlanRevision)}
                        {kv("match layer", entry?.matchLayer)}
                        {kv("similarity", entry?.similarityScore)}
                        {kv("decided by", entry?.decidedBy)}
                      </div>
                      {safeString(entry?.decidedAt) ? (
                        <div className="mt-2 text-xs text-slate-400">
                          decided at {safeString(entry?.decidedAt)}
                        </div>
                      ) : null}
                    </div>
                  ))
                ) : (
                  <div className="rounded-md border border-dashed border-slate-800/80 bg-slate-950/20 p-3 text-xs text-slate-400">
                    No overlap decisions match the current search.
                  </div>
                )}
              </div>
            </div>
          ) : null}
          {approvalPolicy ? (
            <div className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="font-medium text-slate-100">Approval matrix</div>
                <span className="tcp-badge-info">plan level</span>
              </div>
              <div className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
                {planApprovalMatrixRows(approvalPolicy).map((entry) => (
                  <div
                    key={entry.key}
                    className="rounded-md border border-slate-800/80 bg-slate-950/30 p-2"
                  >
                    <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                      {entry.label}
                    </div>
                    <div className="mt-1">
                      <span className="tcp-badge-info">{approvalModeLabel(entry.value)}</span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : null}
          <div className="grid gap-2">
            <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
              Routine history visibility
            </div>
            <div className="grid gap-2">
              {filteredHistoryRoutines.length ? (
                filteredHistoryRoutines.map((routine: any, index: number) => {
                  const routineId = safeString(
                    routine?.routine_id || routine?.id || `routine-${index + 1}`
                  );
                  const ownedContextObjects = contextObjects.filter(
                    (contextObject: any) =>
                      safeString(contextObject?.owner_routine_id) === routineId
                  );
                  return (
                    <div
                      key={routineId}
                      className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="font-medium text-slate-100">{routineId}</div>
                        <span className="tcp-badge-info">
                          {historyVisibilityLabel(routine?.audit_scope?.run_history_visibility)}
                        </span>
                      </div>
                      <div className="mt-2 grid gap-2 sm:grid-cols-3">
                        {kv("history visibility", routine?.audit_scope?.run_history_visibility)}
                        {kv(
                          "intermediate artifacts",
                          routine?.audit_scope?.intermediate_artifact_visibility
                        )}
                        {kv("final artifacts", routine?.audit_scope?.final_artifact_visibility)}
                        {kv("visible context objects", ownedContextObjects.length)}
                      </div>
                      <div className="mt-2">
                        <div className="tcp-subtle tcp-text-caption uppercase tracking-wide">
                          visible context objects
                        </div>
                        <div className="mt-1 break-words text-slate-100">
                          {ownedContextObjects.length
                            ? ownedContextObjects
                                .map((contextObject: any) =>
                                  safeString(
                                    contextObject?.context_object_id || contextObject?.name || ""
                                  )
                                )
                                .filter(Boolean)
                                .join(", ")
                            : "none"}
                        </div>
                      </div>
                    </div>
                  );
                })
              ) : (
                <div className="tcp-subtle text-xs">
                  No routines match the selected history visibility.
                </div>
              )}
            </div>
          </div>
          <LazyJson
            value={validationReport || planValidationState}
            label="Raw validation payload"
            className="rounded-lg border border-slate-800/80 bg-slate-950/30 p-3"
            preClassName="tcp-code mt-2 max-h-64 overflow-auto tcp-text-caption"
          />
        </div>
      ) : null}

      {!routines.length && !credentialEnvelopes.length && !contextObjects.length ? (
        <div className="tcp-subtle">
          Scope data is present, but the plan package does not yet contain routine, credential, or
          handoff details to inspect.
        </div>
      ) : null}
    </div>
  );
}
