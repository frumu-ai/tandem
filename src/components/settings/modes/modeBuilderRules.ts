import type { ModeDefinition, ModeScope, SkillLocation } from "@/lib/tauri";
import { normalizeModeIconId } from "./modeIcons";
import type {
  ModeBuilderAnswers,
  ModeBuilderDraft,
  ModeBuilderEditBoundary,
  ModeBuilderPreset,
} from "./modeBuilderTypes";

const EDIT_TOOLS = new Set([
  "write",
  "write_file",
  "create_file",
  "delete",
  "delete_file",
  "edit",
  "patch",
]);
const TERMINAL_TOOLS = new Set(["bash", "shell", "cmd", "terminal", "run_command"]);
const INTERNET_TOOLS = new Set(["websearch", "webfetch"]);
const VALID_BASE_MODES = new Set(["immediate", "plan", "orchestrate", "coder", "ask", "explore"]);

function unique(values: string[]): string[] {
  return Array.from(new Set(values.map((v) => v.trim()).filter(Boolean)));
}

export function sanitizeModeId(input: string): string {
  const cleaned = input
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");

  if (!cleaned) return "custom-mode";
  if (!/^[a-z]/.test(cleaned)) return `mode-${cleaned}`;
  return cleaned;
}

export function defaultModeScope(workspacePath?: string | null): ModeScope {
  return workspacePath ? "project" : "user";
}

function boundaryGlobs(
  boundary: ModeBuilderEditBoundary,
  customEditGlobs: string
): string[] | undefined {
  if (boundary === "docs") return ["docs/**", "*.md", "README.md"];
  if (boundary === "code") return ["src/**", "tests/**"];
  if (boundary === "custom") {
    const custom = customEditGlobs
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
    return custom.length > 0 ? unique(custom) : undefined;
  }
  return undefined;
}

function filterBySafety(tools: string[], safetyLevel: ModeBuilderAnswers["safetyLevel"]): string[] {
  if (safetyLevel === "power") return tools;

  if (safetyLevel === "balanced") {
    return tools.filter((tool) => !["delete", "delete_file"].includes(tool));
  }

  return tools.filter(
    (tool) =>
      !TERMINAL_TOOLS.has(tool) &&
      !["delete", "delete_file", "patch", "replace", "run_slash_command"].includes(tool)
  );
}

function applyBoundaryRules(
  tools: string[],
  boundary: ModeBuilderEditBoundary,
  safetyLevel: ModeBuilderAnswers["safetyLevel"]
): string[] {
  if (boundary === "none") {
    return tools.filter((tool) => !EDIT_TOOLS.has(tool));
  }

  if (boundary === "project" && safetyLevel === "power") {
    return unique([...tools, "write", "edit", "patch", "create_file", "delete_file"]);
  }

  if (boundary !== "project" && safetyLevel !== "conservative") {
    return unique([...tools, "write", "edit", "create_file"]);
  }

  return tools;
}

function withInternetToggle(tools: string[], allowInternet: boolean): string[] {
  if (allowInternet) return unique([...tools, "websearch", "webfetch"]);
  return tools.filter((tool) => !INTERNET_TOOLS.has(tool));
}

function withTerminalToggle(tools: string[], allowTerminal: boolean): string[] {
  if (allowTerminal) return unique([...tools, "bash", "run_command"]);
  return tools.filter((tool) => !TERMINAL_TOOLS.has(tool));
}

function buildSystemPromptAppend(
  preset: ModeBuilderPreset,
  answers: ModeBuilderAnswers,
  editGlobs?: string[]
): string {
  const segments: string[] = [];
  if (preset.default_system_prompt_append.trim()) {
    segments.push(preset.default_system_prompt_append.trim());
  }

  segments.push(`Safety level: ${answers.safetyLevel}.`);
  segments.push(
    answers.allowInternet
      ? "Internet tools are enabled for research."
      : "Internet tools are disabled; rely on local context."
  );
  segments.push(
    answers.allowTerminal
      ? "Terminal execution is enabled when needed."
      : "Terminal execution is disabled."
  );

  if (answers.editBoundary === "none") {
    segments.push("Do not perform file edit operations.");
  } else if (answers.editBoundary === "project") {
    segments.push("Edits may target project files, with careful scope control.");
  } else if (editGlobs && editGlobs.length > 0) {
    segments.push(`Edit scope is restricted to: ${editGlobs.join(", ")}.`);
  }

  return segments.join(" ");
}

export function buildModeDraft(
  answers: ModeBuilderAnswers,
  preset: ModeBuilderPreset
): ModeBuilderDraft {
  let tools = unique(preset.default_allowed_tools);
  tools = filterBySafety(tools, answers.safetyLevel);
  tools = withInternetToggle(tools, answers.allowInternet);
  tools = withTerminalToggle(tools, answers.allowTerminal);
  tools = applyBoundaryRules(tools, answers.editBoundary, answers.safetyLevel);

  const editGlobs = boundaryGlobs(answers.editBoundary, answers.customEditGlobs);
  const systemPromptAppend = buildSystemPromptAppend(preset, answers, editGlobs);

  const mode: ModeDefinition = {
    id: sanitizeModeId(answers.id || answers.label),
    label: answers.label.trim() || preset.label,
    base_mode: preset.base_mode,
    icon: normalizeModeIconId(answers.icon) ?? preset.default_icon,
    system_prompt_append: systemPromptAppend,
    allowed_tools: tools.length > 0 ? tools : undefined,
    edit_globs:
      answers.editBoundary === "project" || answers.editBoundary === "none" ? undefined : editGlobs,
    auto_approve: false,
  };

  return { mode, scope: answers.scope };
}

export function buildModeBuilderSeedPrompt(
  answers: ModeBuilderAnswers,
  draft: ModeBuilderDraft
): string {
  return [
    "Use the installed `mode-builder` skill to create one Tandem mode.",
    "Ask follow-up questions if anything is ambiguous, then return exactly one final JSON object in a fenced `json` block.",
    "Current choices:",
    `- preset: ${answers.presetId}`,
    `- safety level: ${answers.safetyLevel}`,
    `- edit boundary: ${answers.editBoundary}`,
    `- internet enabled: ${answers.allowInternet}`,
    `- terminal enabled: ${answers.allowTerminal}`,
    `- label: ${answers.label}`,
    `- id: ${sanitizeModeId(answers.id || answers.label)}`,
    `- scope preference: ${answers.scope}`,
    `- icon: ${normalizeModeIconId(answers.icon) ?? draft.mode.icon ?? "zap"}`,
    "Schema fields allowed: id, label, base_mode, system_prompt_append, allowed_tools, edit_globs, auto_approve.",
    "Never include extra keys.",
    "",
    "Reference draft:",
    "```json",
    JSON.stringify(draft.mode, null, 2),
    "```",
  ].join("\n");
}

function extractFirstJsonCodeFence(input: string): string | null {
  const match = input.match(/```json\s*([\s\S]*?)```/i);
  return match?.[1]?.trim() || null;
}

function extractFirstJsonObject(input: string): string | null {
  let depth = 0;
  let start = -1;
  let inString = false;
  let escapeNext = false;

  for (let i = 0; i < input.length; i += 1) {
    const ch = input[i];
    if (inString) {
      if (escapeNext) {
        escapeNext = false;
      } else if (ch === "\\") {
        escapeNext = true;
      } else if (ch === '"') {
        inString = false;
      }
      continue;
    }

    if (ch === '"') {
      inString = true;
      continue;
    }

    if (ch === "{") {
      if (depth === 0) start = i;
      depth += 1;
    } else if (ch === "}") {
      if (depth > 0) depth -= 1;
      if (depth === 0 && start >= 0) {
        return input.slice(start, i + 1);
      }
    }
  }

  return null;
}

function asStringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const normalized = value
    .map((item) => `${item}`)
    .map((item) => item.trim())
    .filter(Boolean);
  return normalized.length > 0 ? unique(normalized) : undefined;
}

export function parseModeFromAiOutput(input: string): ModeDefinition {
  const candidate = extractFirstJsonCodeFence(input) || extractFirstJsonObject(input);
  if (!candidate) {
    throw new Error("No JSON object found. Paste a response that includes one mode JSON object.");
  }

  let parsed: Record<string, unknown>;
  try {
    parsed = JSON.parse(candidate) as Record<string, unknown>;
  } catch (error) {
    throw new Error(`Failed to parse JSON: ${error}`);
  }

  const id = `${parsed.id ?? ""}`.trim();
  const label = `${parsed.label ?? ""}`.trim();
  const baseMode = `${parsed.base_mode ?? ""}`.trim();

  if (!id) throw new Error("Missing required field: id");
  if (!label) throw new Error("Missing required field: label");
  if (!VALID_BASE_MODES.has(baseMode)) {
    throw new Error("Invalid or missing base_mode");
  }

  return {
    id: sanitizeModeId(id),
    label,
    base_mode: baseMode as ModeDefinition["base_mode"],
    icon: normalizeModeIconId(parsed.icon != null ? `${parsed.icon}` : undefined),
    system_prompt_append:
      parsed.system_prompt_append != null ? `${parsed.system_prompt_append}`.trim() : undefined,
    allowed_tools: asStringArray(parsed.allowed_tools),
    edit_globs: asStringArray(parsed.edit_globs),
    auto_approve: Boolean(parsed.auto_approve ?? false),
  };
}

export function skillLocationForWorkspace(workspacePath?: string | null): SkillLocation {
  return workspacePath ? "project" : "global";
}
