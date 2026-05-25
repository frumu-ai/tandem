import type { ModeBase, ModeDefinition, ModeScope } from "@/lib/tauri";

export type ModeBuilderPresetId =
  | "safe-helper"
  | "docs-writer"
  | "research-only"
  | "strict-coder"
  | "code-reviewer"
  | "test-writer"
  | "planner"
  | "orchestrator-assistant"
  | "release-notes"
  | "runbook-assistant"
  | "product-spec"
  | "refactor-guard";

export type ModeBuilderSafetyLevel = "conservative" | "balanced" | "power";

export type ModeBuilderEditBoundary = "none" | "docs" | "code" | "project" | "custom";

export interface ModeBuilderPreset {
  id: ModeBuilderPresetId;
  label: string;
  description: string;
  base_mode: ModeBase;
  default_icon: string;
  default_system_prompt_append: string;
  default_allowed_tools: string[];
  default_edit_boundary: ModeBuilderEditBoundary;
}

export interface ModeBuilderAnswers {
  presetId: ModeBuilderPresetId;
  safetyLevel: ModeBuilderSafetyLevel;
  editBoundary: ModeBuilderEditBoundary;
  allowInternet: boolean;
  allowTerminal: boolean;
  label: string;
  id: string;
  icon: string;
  scope: ModeScope;
  customEditGlobs: string;
  systemPromptAppend: string;
}

export interface ModeBuilderDraft {
  mode: ModeDefinition;
  scope: ModeScope;
}
