import { useMemo } from "react";

type Step1GoalProps = {
  value: string;
  onChange: (v: string) => void;
  routedSkill: string;
  routingConfidence: string;
  validationBadge: string;
  generatedSkill: any;
  advancedMode: boolean;
  customSkillName: string;
  customSkillDescription: string;
  customWorkflowKind: "pack_builder_recipe" | "automation_v2_dag";
  onToggleAdvancedMode: () => void;
  onChangeCustomSkillName: (v: string) => void;
  onChangeCustomSkillDescription: (v: string) => void;
  onChangeCustomWorkflowKind: (v: "pack_builder_recipe" | "automation_v2_dag") => void;
  showArtifactPreview: boolean;
  onToggleArtifactPreview: () => void;
  artifactPreviewKey: string;
  onSelectArtifactPreviewKey: (v: string) => void;
  onGenerateSkill: () => void;
  onInstallGeneratedSkill: () => void;
  isGeneratingSkill: boolean;
  isInstallingSkill: boolean;
  installStatus: string;
  topMatches: Array<{ skill_name?: string; confidence?: number }>;
  isMatching: boolean;
  goalPlaceholder: string;
};

export function Step1Goal(props: Step1GoalProps) {
  const {
    value,
    onChange,
    routedSkill,
    routingConfidence,
    validationBadge,
    generatedSkill,
    advancedMode,
    customSkillName,
    customSkillDescription,
    customWorkflowKind,
    onToggleAdvancedMode,
    onChangeCustomSkillName,
    onChangeCustomSkillDescription,
    onChangeCustomWorkflowKind,
    showArtifactPreview,
    onToggleArtifactPreview,
    artifactPreviewKey,
    onSelectArtifactPreviewKey,
    onGenerateSkill,
    onInstallGeneratedSkill,
    isGeneratingSkill,
    isInstallingSkill,
    installStatus,
    topMatches,
    isMatching,
    goalPlaceholder,
  } = props;

  const generatedArtifactKeys = Object.keys(
    (generatedSkill?.artifacts as Record<string, string>) || {}
  );

  const isMonitorGoal = useMemo(() => {
    const lower = value.toLowerCase();
    return [
      "monitor",
      "watch for",
      "watch our",
      "check for",
      "check my",
      "scan for",
      "scan our",
      "alert me",
      "alert me when",
    ].some((kw) => lower.includes(kw));
  }, [value]);

  return (
    <div className="flex flex-col h-full gap-4 overflow-y-auto">
      <p className="text-sm text-slate-400">
        Describe what you want the AI to do — in plain English. No technical knowledge needed.
      </p>
      <textarea
        className="tcp-input flex-1 min-h-[200px] text-base"
        placeholder={`e.g. "${goalPlaceholder}"`}
        value={value}
        onInput={(e) => onChange((e.target as HTMLTextAreaElement).value)}
        autoFocus
      />
      {isMonitorGoal ? (
        <div className="flex items-start gap-2 rounded-lg border border-blue-500/30 bg-blue-500/10 px-3 py-2 text-xs text-blue-200">
          <span className="mt-0.5 shrink-0 text-base leading-none">⚡</span>
          <span>
            <strong className="text-blue-100">Smart scheduling</strong> — Tandem will use a
            lightweight model to check if there's new work before running the full automation. No
            tokens wasted when there's nothing new.
          </span>
        </div>
      ) : null}
      <div className="rounded-xl border border-slate-700/50 bg-slate-900/30 p-3 text-xs text-slate-300">
        <div className="flex items-center justify-between gap-2">
          <span className="uppercase tracking-wide text-slate-500">Reusable Flows</span>
          <span className="text-slate-500">{isMatching ? "Analyzing…" : "Ready"}</span>
        </div>
        {routedSkill ? (
          <p className="mt-1">
            Reusable flow match: <strong>{routedSkill}</strong>{" "}
            {routingConfidence ? `(${routingConfidence})` : ""}
            {validationBadge ? (
              <span
                className={`ml-2 ${validationBadge === "validated" ? "tcp-badge-ok" : "tcp-badge-warn"}`}
              >
                {validationBadge === "validated" ? "Validated" : "Not validated"}
              </span>
            ) : null}
          </p>
        ) : (
          <p className="mt-1 text-slate-400">
            No reusable flow selected. Tandem will create and run a workflow plan in the engine.
          </p>
        )}
        {topMatches.length ? (
          <div className="mt-2 flex flex-wrap gap-1">
            {topMatches.slice(0, 3).map((m, idx) => (
              <span key={`${String(m?.skill_name || "match")}-${idx}`} className="tcp-badge-info">
                {String(m?.skill_name || "unknown")}{" "}
                {typeof m?.confidence === "number" ? `${Math.round(m.confidence * 100)}%` : ""}
              </span>
            ))}
          </div>
        ) : null}
      </div>
      <div className="rounded-xl border border-slate-700/50 bg-slate-900/30 p-3 text-xs text-slate-300">
        <div className="flex items-center justify-between gap-2">
          <span className="uppercase tracking-wide text-slate-500">
            Optional: Reusable Skill Export
          </span>
          <div className="flex items-center gap-2">
            <button className="tcp-btn h-7 px-2 text-xs" onClick={onToggleAdvancedMode}>
              {advancedMode ? "Hide Export Options" : "Show Export Options"}
            </button>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              onClick={onGenerateSkill}
              disabled={!value.trim() || isGeneratingSkill}
            >
              {isGeneratingSkill ? "Generating…" : "Generate Reusable Skill Draft"}
            </button>
            <button
              className="tcp-btn h-7 px-2 text-xs"
              onClick={onInstallGeneratedSkill}
              disabled={!generatedSkill?.artifacts || isInstallingSkill}
            >
              {isInstallingSkill ? "Installing…" : "Save Reusable Skill"}
            </button>
          </div>
        </div>
        <p className="mt-1 text-slate-400">
          This is a secondary prompt-based export path. It does not power the default automation
          flow, and it does not automatically track planner-chat revisions to the workflow plan.
        </p>
        <div className="mt-2 rounded-lg border border-slate-800/70 bg-slate-950/30 px-3 py-2 text-xs text-slate-400">
          Recommended flow: review and finalize the workflow plan first, then generate or regenerate
          the reusable skill draft if you want a reusable export from the current prompt.
        </div>
        {advancedMode ? (
          <div className="mt-2 grid gap-2">
            <input
              className="tcp-input text-xs"
              placeholder="skill-name"
              value={customSkillName}
              onInput={(e) => onChangeCustomSkillName((e.target as HTMLInputElement).value)}
            />
            <input
              className="tcp-input text-xs"
              placeholder="Short skill description"
              value={customSkillDescription}
              onInput={(e) => onChangeCustomSkillDescription((e.target as HTMLInputElement).value)}
            />
            <select
              className="tcp-input text-xs"
              value={customWorkflowKind}
              onInput={(e) =>
                onChangeCustomWorkflowKind(
                  (e.target as HTMLSelectElement).value as
                    | "pack_builder_recipe"
                    | "automation_v2_dag"
                )
              }
            >
              <option value="pack_builder_recipe">pack_builder_recipe</option>
              <option value="automation_v2_dag">automation_v2_dag</option>
            </select>
          </div>
        ) : null}
        {generatedSkill ? (
          <div className="mt-2 grid gap-1">
            <p>
              Optional scaffold status:{" "}
              <strong>{String(generatedSkill?.status || "generated")}</strong>
            </p>
            <p className="text-amber-200">
              This draft was generated from the prompt and export options. If you revise the
              workflow plan later in review, regenerate this draft before saving it.
            </p>
            <p>
              Suggested skill:{" "}
              <strong>{String(generatedSkill?.router?.skill_name || "new optional skill")}</strong>
            </p>
            <p className="text-slate-400">
              Artifacts:{" "}
              {generatedArtifactKeys.join(", ") ||
                "SKILL.md, workflow.yaml, automation.example.yaml"}
            </p>
            <div className="mt-1 flex items-center gap-2">
              <button className="tcp-btn h-7 px-2 text-xs" onClick={onToggleArtifactPreview}>
                {showArtifactPreview ? "Hide Raw" : "Show Raw"}
              </button>
              {showArtifactPreview ? (
                <select
                  className="tcp-input h-7 text-xs"
                  value={artifactPreviewKey}
                  onInput={(e) => onSelectArtifactPreviewKey((e.target as HTMLSelectElement).value)}
                >
                  {Object.keys((generatedSkill?.artifacts as Record<string, string>) || {}).map(
                    (key) => (
                      <option key={key} value={key}>
                        {key}
                      </option>
                    )
                  )}
                </select>
              ) : null}
            </div>
            {showArtifactPreview ? (
              <textarea
                className="tcp-input min-h-[140px] font-mono text-[11px]"
                readOnly
                value={String(
                  (generatedSkill?.artifacts as Record<string, string>)?.[artifactPreviewKey] || ""
                )}
              />
            ) : null}
          </div>
        ) : (
          <p className="mt-1 text-slate-400">
            Generate a reusable skill draft from this prompt if you want to save it for later reuse.
          </p>
        )}
        {installStatus ? <p className="mt-2 text-slate-300">{installStatus}</p> : null}
      </div>
    </div>
  );
}
