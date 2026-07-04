import { Badge, PanelCard } from "../../ui/index.tsx";
import type { IntentBriefDraft } from "./IntentBriefPanel";
import { buildKnowledgeRolloutGuidance } from "./plannerShared";
import { PlannerMetricGrid, PlannerSubsection } from "./plannerPrimitives";

function safeString(value: unknown) {
  return String(value || "").trim();
}

function asObject(value: unknown): Record<string, any> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, any>)
    : {};
}

function extractKnowledge(step: any) {
  const fromNode = asObject(step?.knowledge);
  if (Object.keys(fromNode).length) return fromNode;
  const metadata = asObject(step?.metadata);
  const builder = asObject(metadata.builder);
  const knowledge = asObject(builder.knowledge);
  return Object.keys(knowledge).length ? knowledge : null;
}

function safeListLabel(values: unknown, fallback = "n/a") {
  const rows = Array.isArray(values)
    ? values
        .map((entry) => {
          if (entry && typeof entry === "object") {
            const scope = safeString((entry as any).scope);
            const namespace = safeString((entry as any).namespace);
            if (scope && namespace) return `${scope}:${namespace}`;
            if (scope) return scope;
          }
          return safeString(entry);
        })
        .filter(Boolean)
    : [];
  return rows.length ? rows.join(", ") : fallback;
}

export function PlanSummaryPanel({
  planPreview,
  brief,
  planPackage,
  planPackageBundle,
}: {
  planPreview: any;
  brief: IntentBriefDraft;
  planPackage: any;
  planPackageBundle: any;
}) {
  const steps = Array.isArray(planPreview?.steps) ? planPreview.steps : [];
  const routines = Array.isArray(planPackage?.routine_graph) ? planPackage.routine_graph : [];
  const bundleSnapshot = asObject(
    planPackageBundle?.scope_snapshot || planPackageBundle?.scopeSnapshot
  );
  const outputRoots = asObject(bundleSnapshot.output_roots || bundleSnapshot.outputRoots);
  const knowledgeDefaults = steps.map(extractKnowledge).find(Boolean) || null;
  const knowledgeRollout = buildKnowledgeRolloutGuidance(brief.goal).rollout;

  return (
    <PanelCard
      title="Plan summary"
      subtitle="Review the compiler draft and bundle snapshot before handing it off downstream."
    >
      {planPreview ? (
        <div className="grid gap-3 text-sm">
          <div className="flex flex-wrap gap-2">
            <Badge tone={planPackage ? "ok" : "warn"}>
              {planPackage ? "plan package ready" : "plan package pending"}
            </Badge>
            <Badge tone={planPackageBundle ? "ok" : "warn"}>
              {planPackageBundle ? "bundle ready" : "bundle pending"}
            </Badge>
            <Badge tone="info">{brief.targetSurface.replace("_", " ")}</Badge>
            <Badge tone="info">{brief.planningHorizon.replace("_", " ")}</Badge>
          </div>

          <PlannerMetricGrid
            metrics={[
              {
                label: "Plan title",
                value:
                  safeString(planPreview?.title) || safeString(planPreview?.plan_id) || "Draft",
              },
              {
                label: "Plan id",
                value: safeString(planPreview?.plan_id || planPreview?.planId) || "pending",
              },
              {
                label: "Routines",
                value: routines.length || "pending",
              },
              {
                label: "Steps",
                value: steps.length || "pending",
              },
            ]}
          />

          <PlannerSubsection title="Description">
            <div className="text-slate-200">
              {safeString(planPreview?.description) || "No description returned yet."}
            </div>
          </PlannerSubsection>

          <PlannerSubsection
            title="Knowledge defaults"
            description="The compiler now carries reusable knowledge defaults alongside each step."
          >
            {knowledgeDefaults ? (
              <div className="grid gap-2 text-xs text-slate-200">
                <div className="flex flex-wrap gap-2">
                  <Badge tone={knowledgeDefaults.enabled === false ? "warn" : "ok"}>
                    {knowledgeDefaults.enabled === false ? "knowledge off" : "knowledge on"}
                  </Badge>
                  <Badge tone="info">
                    reuse: {safeString(knowledgeDefaults.reuse_mode || "preflight")}
                  </Badge>
                  <Badge tone="info">
                    trust: {safeString(knowledgeDefaults.trust_floor || "promoted")}
                  </Badge>
                </div>
                <div className="tcp-subtle text-xs">
                  subject:{" "}
                  {safeString(knowledgeDefaults.subject || planPreview?.original_prompt || "n/a")}
                </div>
                <div className="tcp-subtle text-xs">
                  reads: {safeListLabel(knowledgeDefaults.read_spaces, "project")}
                </div>
                <div className="tcp-subtle text-xs">
                  promotes: {safeListLabel(knowledgeDefaults.promote_spaces, "project")}
                </div>
              </div>
            ) : (
              <div className="tcp-subtle text-xs">
                No knowledge defaults surfaced yet; the plan compiler may not have normalized the
                step metadata.
              </div>
            )}
          </PlannerSubsection>

          <PlannerSubsection
            title="Rollout guardrails"
            description="Use these checks when expanding reusable knowledge across workflows."
          >
            <div className="grid gap-2 text-xs text-slate-200">
              <div className="flex flex-wrap gap-2">
                <Badge tone="warn">project-first pilot</Badge>
                <Badge tone="warn">promoted only</Badge>
                <Badge tone="warn">approved_default rare</Badge>
              </div>
              <div className="tcp-subtle">
                {knowledgeRollout.rollout_mode === "project_first_pilot"
                  ? "Run a project-scoped pilot first, then widen only after the reuse path is stable."
                  : "Rollout guidance is missing or incomplete."}
              </div>
              <ul className="space-y-1">
                {knowledgeRollout.guardrails.map((item: string) => (
                  <li key={item}>• {item}</li>
                ))}
              </ul>
            </div>
          </PlannerSubsection>

          <PlannerSubsection title="Bundle snapshot">
            <PlannerMetricGrid
              metrics={[
                {
                  label: "Bundle version",
                  value:
                    safeString(
                      planPackageBundle?.bundle_version || planPackageBundle?.bundleVersion
                    ) || "pending",
                },
                {
                  label: "Snapshot plan",
                  value: safeString(bundleSnapshot.plan_id || bundleSnapshot.planId) || "pending",
                },
                {
                  label: "Snapshot revision",
                  value:
                    safeString(bundleSnapshot.plan_revision || bundleSnapshot.planRevision) ||
                    "pending",
                },
                {
                  label: "Output roots",
                  value: (
                    <div className="text-xs text-slate-200">
                      {outputRoots.plan ? <div>plan: {safeString(outputRoots.plan)}</div> : null}
                      {outputRoots.history ? (
                        <div>history: {safeString(outputRoots.history)}</div>
                      ) : null}
                      {outputRoots.proof ? <div>proof: {safeString(outputRoots.proof)}</div> : null}
                      {outputRoots.drafts ? (
                        <div>drafts: {safeString(outputRoots.drafts)}</div>
                      ) : null}
                      {!outputRoots.plan &&
                      !outputRoots.history &&
                      !outputRoots.proof &&
                      !outputRoots.drafts ? (
                        <div className="tcp-subtle">No bundle roots available yet.</div>
                      ) : null}
                    </div>
                  ),
                },
              ]}
            />
          </PlannerSubsection>

          <PlannerSubsection title="Step graph">
            <div className="grid gap-2">
              {steps.length ? (
                steps.map((step: any, index: number) => (
                  <div
                    key={safeString(step?.step_id || step?.stepId || index)}
                    className="rounded-lg border border-white/10 bg-black/20 p-3"
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <Badge tone="info">Step {index + 1}</Badge>
                      <span className="text-sm font-medium text-slate-100">
                        {safeString(step?.objective) || safeString(step?.title) || "Untitled step"}
                      </span>
                    </div>
                    <div className="tcp-subtle mt-2 text-xs">
                      role: {safeString(step?.agent_role || step?.agentRole) || "n/a"}
                    </div>
                    <div className="tcp-subtle mt-1 text-xs">
                      depends on:{" "}
                      {Array.isArray(step?.depends_on) && step.depends_on.length
                        ? step.depends_on.join(", ")
                        : "none"}
                    </div>
                    {extractKnowledge(step) ? (
                      <div className="mt-2 flex flex-wrap gap-2 tcp-text-caption">
                        <Badge tone={extractKnowledge(step)?.enabled === false ? "warn" : "ok"}>
                          {extractKnowledge(step)?.enabled === false
                            ? "knowledge off"
                            : "knowledge on"}
                        </Badge>
                        <Badge tone="info">
                          reuse: {safeString(extractKnowledge(step)?.reuse_mode || "preflight")}
                        </Badge>
                        <Badge tone="info">
                          trust: {safeString(extractKnowledge(step)?.trust_floor || "promoted")}
                        </Badge>
                        <span className="tcp-subtle tcp-text-caption">
                          subject: {safeString(extractKnowledge(step)?.subject || "inferred")}
                        </span>
                      </div>
                    ) : null}
                  </div>
                ))
              ) : (
                <div className="tcp-subtle text-xs">No step graph has been returned yet.</div>
              )}
            </div>
          </PlannerSubsection>
        </div>
      ) : (
        <div className="tcp-subtle text-sm">
          No plan yet. Start with the intent brief and planner chat to generate a draft.
        </div>
      )}
    </PanelCard>
  );
}
