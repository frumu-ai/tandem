import { useMemo, useState } from "react";
import { Badge, PanelCard } from "../../ui/index.tsx";
import type { IntentBriefDraft } from "./IntentBriefPanel";

const AUTOMATION_PLANNER_SEED_KEY = "tandem.automations.plannerSeed";
const CODING_TASK_PLANNING_HANDOFF_KEY = "tandem.intent-planner.codingTaskHandoff.v1";

function safeString(value: unknown) {
  return String(value || "").trim();
}

function buildCodingHandoffPayload({
  brief,
  planPreview,
  planPackageBundle,
  planPackage,
  validationReport,
  overlapAnalysis,
  teachingLibrary,
}: {
  brief: IntentBriefDraft;
  planPreview: any;
  planPackageBundle: any;
  planPackage: any;
  validationReport: any;
  overlapAnalysis: any;
  teachingLibrary: any;
}) {
  return {
    goal: safeString(brief.goal || planPreview?.description || planPreview?.title),
    workspaceRoot: safeString(brief.workspaceRoot),
    notes: [
      `Target surface: ${brief.targetSurface.replace("_", " ")}`,
      `Planning horizon: ${brief.planningHorizon.replace("_", " ")}`,
      brief.outputExpectations ? `Expected outputs: ${safeString(brief.outputExpectations)}` : "",
      brief.constraints ? `Constraints: ${safeString(brief.constraints)}` : "",
    ]
      .filter(Boolean)
      .join("\n"),
    plannerProvider: safeString(brief.plannerProvider),
    plannerModel: safeString(brief.plannerModel),
    plan: planPackage || planPreview || null,
    conversation: null,
    changeSummary: [],
    plannerError: "",
    plannerDiagnostics: {
      validation_report: validationReport || null,
      overlap_analysis: overlapAnalysis || null,
      teaching_library: teachingLibrary || null,
      handoff_source: "intent_planner_page",
    },
    publishedAtMs: null,
    publishedTasks: [],
    planPackageBundle: planPackageBundle || null,
  };
}

export function PlannerHandoffPanel({
  brief,
  planPreview,
  planPackage,
  planPackageBundle,
  validationReport,
  overlapAnalysis,
  teachingLibrary,
  navigate,
  toast,
}: {
  brief: IntentBriefDraft;
  planPreview: any;
  planPackage: any;
  planPackageBundle: any;
  validationReport: any;
  overlapAnalysis: any;
  teachingLibrary: any;
  navigate: (route: string) => void;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
}) {
  const [copyStatus, setCopyStatus] = useState("");
  const canExport = !!planPackageBundle;
  const canSeed = !!planPreview || !!planPackage;

  const codingHandoffPayload = useMemo(
    () =>
      buildCodingHandoffPayload({
        brief,
        planPreview,
        planPackageBundle,
        planPackage,
        validationReport,
        overlapAnalysis,
        teachingLibrary,
      }),
    [
      brief,
      overlapAnalysis,
      planPackage,
      planPackageBundle,
      planPreview,
      teachingLibrary,
      validationReport,
    ]
  );

  const seedAutomationDraft = () => {
    const prompt = safeString(brief.goal || planPreview?.description || planPreview?.title);
    if (!prompt) {
      toast("warn", "Add an intent first so we have something to hand off.");
      return;
    }
    try {
      sessionStorage.setItem(
        AUTOMATION_PLANNER_SEED_KEY,
        JSON.stringify({
          prompt,
          plan_source: "intent_planner_page",
        })
      );
      navigate("automations");
    } catch {
      toast("err", "Unable to seed the automation draft handoff.");
    }
  };

  const seedCodingBundle = () => {
    if (!canSeed) {
      toast("warn", "Generate a plan before publishing a coding task bundle.");
      return;
    }
    try {
      sessionStorage.setItem(
        CODING_TASK_PLANNING_HANDOFF_KEY,
        JSON.stringify(codingHandoffPayload)
      );
      navigate("coding");
    } catch {
      toast("err", "Unable to seed the coding handoff.");
    }
  };

  const exportBundle = async () => {
    if (!canExport) {
      toast("warn", "Generate a bundle before exporting it.");
      return;
    }
    try {
      const payload = JSON.stringify(planPackageBundle, null, 2);
      if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(payload);
        setCopyStatus("Bundle copied to clipboard.");
        toast("ok", "Plan bundle copied to clipboard.");
      } else {
        setCopyStatus("Clipboard unavailable.");
        toast("warn", "Clipboard unavailable in this environment.");
      }
    } catch {
      setCopyStatus("Copy failed.");
      toast("err", "Unable to copy the plan bundle.");
    }
  };

  return (
    <PanelCard
      title="Handoff"
      subtitle="Send the plan downstream without rebuilding it in another page."
    >
      <div className="grid gap-3 text-sm">
        <div className="flex flex-wrap gap-2">
          <Badge tone={canSeed ? "ok" : "warn"}>
            {canSeed ? "handoff ready" : "handoff pending"}
          </Badge>
          <Badge tone={canExport ? "ok" : "warn"}>
            {canExport ? "bundle ready" : "bundle pending"}
          </Badge>
          <Badge tone="info">{brief.targetSurface.replace("_", " ")}</Badge>
        </div>

        <div className="flex flex-wrap gap-2">
          <button type="button" className="tcp-btn" onClick={seedAutomationDraft}>
            <i data-lucide="bot" className="mr-1 h-3 w-3"></i>
            Create automation draft
          </button>
          <button type="button" className="tcp-btn" onClick={() => navigate("studio")}>
            <i data-lucide="network" className="mr-1 h-3 w-3"></i>
            Open in mission builder
          </button>
          <button type="button" className="tcp-btn" onClick={seedCodingBundle}>
            <i data-lucide="code" className="mr-1 h-3 w-3"></i>
            Publish coding task bundle
          </button>
          <button type="button" className="tcp-btn" onClick={() => navigate("orchestrator")}>
            <i data-lucide="sparkles" className="mr-1 h-3 w-3"></i>
            Open in orchestrator
          </button>
          <button type="button" className="tcp-btn-primary" onClick={() => void exportBundle()}>
            <i data-lucide="download" className="mr-1 h-3 w-3"></i>
            Export plan bundle
          </button>
        </div>

        {copyStatus ? <div className="tcp-subtle text-xs">{copyStatus}</div> : null}
      </div>
    </PanelCard>
  );
}
