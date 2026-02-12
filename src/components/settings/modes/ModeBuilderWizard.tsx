import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";
import { useModes } from "@/hooks/useModes";
import { installSkillTemplate, upsertMode } from "@/lib/tauri";
import { getModeBuilderPreset, MODE_BUILDER_PRESETS } from "./modeBuilderPresets";
import { ModeIconPicker } from "./ModeIconPicker";
import {
  buildModeBuilderSeedPrompt,
  buildModeDraft,
  defaultModeScope,
  parseModeFromAiOutput,
  sanitizeModeId,
  skillLocationForWorkspace,
} from "./modeBuilderRules";
import type {
  ModeBuilderAnswers,
  ModeBuilderEditBoundary,
  ModeBuilderSafetyLevel,
} from "./modeBuilderTypes";

interface ModeBuilderWizardProps {
  workspacePath?: string | null;
  onStartModeBuilderChat?: (seedPrompt: string) => void;
}

const STEPS = ["Preset", "Safety", "Edit Boundary", "Internet", "Terminal", "Identity", "Preview"];

function defaultAnswers(workspacePath?: string | null): ModeBuilderAnswers {
  const preset = MODE_BUILDER_PRESETS[0];
  return {
    presetId: preset.id,
    safetyLevel: "balanced",
    editBoundary: preset.default_edit_boundary,
    allowInternet: true,
    allowTerminal: false,
    label: preset.label,
    id: sanitizeModeId(preset.label),
    icon: preset.default_icon,
    scope: defaultModeScope(workspacePath),
    customEditGlobs: "",
    systemPromptAppend: preset.default_system_prompt_append,
  };
}

export function ModeBuilderWizard({
  workspacePath,
  onStartModeBuilderChat,
}: ModeBuilderWizardProps) {
  const { refreshModes } = useModes();
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<ModeBuilderAnswers>(() => defaultAnswers(workspacePath));
  const [idManuallyEdited, setIdManuallyEdited] = useState(false);
  const [status, setStatus] = useState<string>("");
  const [isSaving, setIsSaving] = useState(false);
  const [isStartingAi, setIsStartingAi] = useState(false);
  const [aiOutput, setAiOutput] = useState("");
  const [aiAssistOpen, setAiAssistOpen] = useState(false);
  const [parsedMode, setParsedMode] = useState<ReturnType<typeof parseModeFromAiOutput> | null>(
    null
  );

  const preset = useMemo(() => getModeBuilderPreset(answers.presetId), [answers.presetId]);
  const generatedDraft = useMemo(() => buildModeDraft(answers, preset), [answers, preset]);
  const draftMode = parsedMode ?? generatedDraft.mode;

  useEffect(() => {
    if (workspacePath) return;
    setAnswers((prev) => (prev.scope === "project" ? { ...prev, scope: "user" } : prev));
  }, [workspacePath]);

  const updateSafetyLevel = (safetyLevel: ModeBuilderSafetyLevel) => {
    setParsedMode(null);
    setAnswers((prev) => ({ ...prev, safetyLevel }));
  };

  const updateEditBoundary = (editBoundary: ModeBuilderEditBoundary) => {
    setParsedMode(null);
    setAnswers((prev) => ({ ...prev, editBoundary }));
  };

  const handlePresetSelect = (presetId: ModeBuilderAnswers["presetId"]) => {
    const nextPreset = getModeBuilderPreset(presetId);
    setParsedMode(null);
    setAnswers((prev) => ({
      ...prev,
      presetId,
      editBoundary: nextPreset.default_edit_boundary,
      systemPromptAppend: nextPreset.default_system_prompt_append,
      icon: nextPreset.default_icon,
      label: idManuallyEdited ? prev.label : nextPreset.label,
      id: idManuallyEdited ? prev.id : sanitizeModeId(nextPreset.label),
    }));
  };

  const handleStartAiBuilder = async () => {
    const seedPrompt = buildModeBuilderSeedPrompt(answers, generatedDraft);
    const location = skillLocationForWorkspace(workspacePath);

    setIsStartingAi(true);
    setStatus("");
    try {
      await installSkillTemplate("mode-builder", location);
      setStatus("Installed mode-builder skill template. Opening chat with a seeded draft prompt.");
    } catch (error) {
      setStatus(
        `Could not auto-install mode-builder skill (${error}). Opening chat anyway with a fallback prompt.`
      );
    } finally {
      setIsStartingAi(false);
    }

    onStartModeBuilderChat?.(seedPrompt);
  };

  const handleParseAiOutput = () => {
    setStatus("");
    try {
      const parsed = parseModeFromAiOutput(aiOutput);
      setParsedMode(parsed);
      setAnswers((prev) => ({
        ...prev,
        id: parsed.id,
        label: parsed.label,
        icon: parsed.icon ?? prev.icon,
      }));
      setIdManuallyEdited(true);
      setStep(6);
      setStatus("Parsed AI output successfully. Review the preview before applying.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Failed to parse AI output");
    }
  };

  const handleApply = async () => {
    setIsSaving(true);
    setStatus("");
    try {
      const payload = {
        ...draftMode,
        id: sanitizeModeId(answers.id || draftMode.id),
        label: answers.label.trim() || draftMode.label,
      };
      await upsertMode(answers.scope, payload);
      await refreshModes();
      setStatus(`Saved mode '${payload.id}' to ${answers.scope} scope.`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Failed to save mode");
    } finally {
      setIsSaving(false);
    }
  };

  const canGoNext = useMemo(() => {
    if (step === 5) {
      return answers.label.trim().length > 0 && sanitizeModeId(answers.id).length > 0;
    }
    return true;
  }, [answers.id, answers.label, step]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Guided Builder (Recommended)</CardTitle>
        <CardDescription>
          Build a custom mode with simple questions. Tandem converts your choices into a safe,
          backend-validated mode definition.
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-4">
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          {STEPS.map((name, index) => (
            <div
              key={name}
              className={`rounded-md border px-2 py-1 text-xs ${
                step === index
                  ? "border-primary bg-primary/10 text-primary"
                  : "border-border bg-surface text-text-muted"
              }`}
            >
              {index + 1}. {name}
            </div>
          ))}
        </div>

        {step === 0 && (
          <div className="space-y-2">
            <p className="text-sm text-text-muted">
              Choose a starting preset. You can still customize behavior in later steps.
            </p>
            <div className="grid gap-2 md:grid-cols-2">
              {MODE_BUILDER_PRESETS.map((candidate) => (
                <button
                  key={candidate.id}
                  type="button"
                  onClick={() => handlePresetSelect(candidate.id)}
                  className={`rounded-md border p-3 text-left transition-colors ${
                    answers.presetId === candidate.id
                      ? "border-primary bg-primary/10"
                      : "border-border bg-surface hover:bg-surface-elevated"
                  }`}
                >
                  <p className="text-sm font-medium text-text">{candidate.label}</p>
                  <p className="mt-1 text-xs text-text-muted">{candidate.description}</p>
                </button>
              ))}
            </div>
          </div>
        )}

        {step === 1 && (
          <div className="space-y-2">
            <p className="text-sm text-text-muted">
              Safety level controls how aggressive tool usage can be.
            </p>
            <div className="grid gap-2 sm:grid-cols-3">
              {[
                ["conservative", "Safest defaults, minimizes risky edits."],
                ["balanced", "Allows practical edits while reducing risky actions."],
                ["power", "Full capability profile for advanced workflows."],
              ].map(([value, desc]) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => updateSafetyLevel(value as ModeBuilderSafetyLevel)}
                  className={`rounded-md border p-3 text-left ${
                    answers.safetyLevel === value
                      ? "border-primary bg-primary/10"
                      : "border-border bg-surface hover:bg-surface-elevated"
                  }`}
                >
                  <p className="text-sm font-medium text-text capitalize">{value}</p>
                  <p className="mt-1 text-xs text-text-muted">{desc}</p>
                </button>
              ))}
            </div>
          </div>
        )}

        {step === 2 && (
          <div className="space-y-3">
            <p className="text-sm text-text-muted">
              Define where edits are allowed. Tandem enforces this server-side.
            </p>
            <div className="grid gap-2 sm:grid-cols-2">
              {[
                ["none", "Read-only mode, no file edits."],
                ["docs", "Docs + markdown only."],
                ["code", "Source + tests only."],
                ["project", "Any project path."],
                ["custom", "You define glob patterns."],
              ].map(([value, desc]) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => updateEditBoundary(value as ModeBuilderEditBoundary)}
                  className={`rounded-md border p-3 text-left ${
                    answers.editBoundary === value
                      ? "border-primary bg-primary/10"
                      : "border-border bg-surface hover:bg-surface-elevated"
                  }`}
                >
                  <p className="text-sm font-medium text-text capitalize">{value}</p>
                  <p className="mt-1 text-xs text-text-muted">{desc}</p>
                </button>
              ))}
            </div>

            {answers.editBoundary === "custom" && (
              <div>
                <label className="text-xs font-medium text-text-muted">
                  Custom edit globs (one per line)
                </label>
                <textarea
                  className="mt-1 min-h-[90px] w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
                  value={answers.customEditGlobs}
                  onChange={(event) => {
                    setParsedMode(null);
                    setAnswers((prev) => ({ ...prev, customEditGlobs: event.target.value }));
                  }}
                  placeholder="src/**&#10;docs/**/*.md"
                />
              </div>
            )}
          </div>
        )}

        {step === 3 && (
          <div className="space-y-2">
            <p className="text-sm text-text-muted">
              Enable web tools for external research and references.
            </p>
            <div className="flex gap-2">
              <Button
                variant={answers.allowInternet ? "primary" : "secondary"}
                onClick={() => {
                  setParsedMode(null);
                  setAnswers((prev) => ({ ...prev, allowInternet: true }));
                }}
              >
                Internet On
              </Button>
              <Button
                variant={!answers.allowInternet ? "primary" : "secondary"}
                onClick={() => {
                  setParsedMode(null);
                  setAnswers((prev) => ({ ...prev, allowInternet: false }));
                }}
              >
                Internet Off
              </Button>
            </div>
          </div>
        )}

        {step === 4 && (
          <div className="space-y-2">
            <p className="text-sm text-text-muted">
              Enable terminal tools only if users need local command execution.
            </p>
            <div className="flex gap-2">
              <Button
                variant={answers.allowTerminal ? "primary" : "secondary"}
                onClick={() => {
                  setParsedMode(null);
                  setAnswers((prev) => ({ ...prev, allowTerminal: true }));
                }}
              >
                Terminal On
              </Button>
              <Button
                variant={!answers.allowTerminal ? "primary" : "secondary"}
                onClick={() => {
                  setParsedMode(null);
                  setAnswers((prev) => ({ ...prev, allowTerminal: false }));
                }}
              >
                Terminal Off
              </Button>
            </div>
          </div>
        )}

        {step === 5 && (
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div>
              <label className="text-xs font-medium text-text-muted">Mode label</label>
              <Input
                value={answers.label}
                onChange={(event) => {
                  const nextLabel = event.target.value;
                  setParsedMode(null);
                  setAnswers((prev) => ({
                    ...prev,
                    label: nextLabel,
                    id: idManuallyEdited ? prev.id : sanitizeModeId(nextLabel),
                  }));
                }}
                placeholder="Safe Helper"
              />
            </div>

            <div>
              <label className="text-xs font-medium text-text-muted">Mode ID (kebab-case)</label>
              <Input
                value={answers.id}
                onChange={(event) => {
                  setParsedMode(null);
                  setIdManuallyEdited(true);
                  setAnswers((prev) => ({ ...prev, id: sanitizeModeId(event.target.value) }));
                }}
                placeholder="safe-helper"
              />
            </div>

            <div className="sm:col-span-2">
              <label className="text-xs font-medium text-text-muted">Mode icon</label>
              <ModeIconPicker
                value={answers.icon}
                onChange={(nextIcon) => {
                  setParsedMode(null);
                  setAnswers((prev) => ({ ...prev, icon: nextIcon }));
                }}
              />
            </div>

            <div className="sm:col-span-2">
              <label className="text-xs font-medium text-text-muted">Save scope</label>
              <select
                className="mt-1 w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
                value={answers.scope}
                onChange={(event) =>
                  setAnswers((prev) => ({
                    ...prev,
                    scope: event.target.value as ModeBuilderAnswers["scope"],
                  }))
                }
              >
                <option value="project" disabled={!workspacePath}>
                  Project
                </option>
                <option value="user">User</option>
              </select>
            </div>
          </div>
        )}

        {step === 6 && (
          <div className="space-y-3">
            <p className="text-sm text-text-muted">
              Preview the final payload before applying. Backend validation is always authoritative.
            </p>

            <div className="rounded-md border border-border bg-surface-elevated p-3">
              <div className="mb-2 flex items-center justify-between">
                <p className="text-xs font-medium text-text-muted">
                  Preview source: {parsedMode ? "Parsed AI output" : "Guided builder"}
                </p>
                <p className="text-xs text-text-subtle">Scope: {answers.scope}</p>
              </div>
              <pre className="max-h-[280px] overflow-auto rounded bg-surface p-3 text-xs text-text">
                {JSON.stringify(
                  {
                    ...draftMode,
                    id: sanitizeModeId(answers.id || draftMode.id),
                    label: answers.label.trim() || draftMode.label,
                  },
                  null,
                  2
                )}
              </pre>
            </div>

            <div className="rounded-md border border-border bg-surface-elevated/40 p-3">
              <div className="flex items-center justify-between gap-2">
                <p className="text-sm font-medium text-text">AI Assist</p>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setAiAssistOpen((prev) => !prev)}
                  disabled={isSaving}
                >
                  {aiAssistOpen ? "Hide" : "Show"}
                </Button>
              </div>

              {aiAssistOpen && (
                <div className="mt-2 space-y-2">
                  <p className="text-xs text-text-muted">
                    Launch a guided AI chat, then paste the resulting JSON here to preview before
                    apply.
                  </p>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      variant="secondary"
                      onClick={handleStartAiBuilder}
                      loading={isStartingAi}
                      disabled={isSaving}
                    >
                      Start AI Builder Chat
                    </Button>
                  </div>
                  <textarea
                    className="min-h-[120px] w-full rounded-md border border-border bg-surface px-3 py-2 font-mono text-xs text-text"
                    value={aiOutput}
                    onChange={(event) => setAiOutput(event.target.value)}
                    placeholder="Paste AI output containing one JSON object..."
                  />
                  <div className="flex gap-2">
                    <Button
                      variant="secondary"
                      onClick={handleParseAiOutput}
                      disabled={!aiOutput.trim()}
                    >
                      Parse + Preview
                    </Button>
                    <Button
                      variant="ghost"
                      onClick={() => {
                        setAiOutput("");
                        setParsedMode(null);
                      }}
                      disabled={!aiOutput.trim() && !parsedMode}
                    >
                      Clear AI Data
                    </Button>
                  </div>
                </div>
              )}
              {!aiAssistOpen && (
                <p className="mt-2 text-xs text-text-subtle">
                  Optional: use AI Assist for more custom configurations.
                </p>
              )}
            </div>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="ghost"
            onClick={() => setStep((prev) => Math.max(0, prev - 1))}
            disabled={step === 0}
          >
            Back
          </Button>
          {step < STEPS.length - 1 ? (
            <Button
              onClick={() => setStep((prev) => Math.min(STEPS.length - 1, prev + 1))}
              disabled={!canGoNext}
            >
              Next
            </Button>
          ) : (
            <Button onClick={handleApply} loading={isSaving}>
              Apply Mode
            </Button>
          )}
          <Button
            variant="ghost"
            onClick={() => {
              setStep(0);
              setParsedMode(null);
              setIdManuallyEdited(false);
              setStatus("");
              setAiOutput("");
              setAiAssistOpen(false);
              setAnswers(defaultAnswers(workspacePath));
            }}
            disabled={isSaving || isStartingAi}
          >
            Reset Wizard
          </Button>
        </div>

        {status && <p className="text-xs text-text-muted">{status}</p>}
      </CardContent>
    </Card>
  );
}
