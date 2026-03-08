import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { useEngineStream } from "../features/stream/useEngineStream";
import { PageCard, EmptyState, formatJson } from "./ui";
import type { AppPageProps } from "./pageTypes";

// ─── Types ─────────────────────────────────────────────────────────────────

type ExecutionMode = "single" | "team" | "swarm";
type WizardStep = 1 | 2 | 3 | 4;
type ActiveTab = "create" | "list" | "running" | "approvals";

interface SchedulePreset {
  label: string;
  desc: string;
  icon: string;
  cron: string;
  intervalSeconds?: number;
}

interface WizardState {
  goal: string;
  workspaceRoot: string;
  schedulePreset: string;
  cron: string;
  mode: ExecutionMode;
  maxAgents: string;
  routedSkill: string;
  routingConfidence: string;
  modelProvider: string;
  modelId: string;
  roleModelsJson: string;
  selectedMcpServers: string[];
  exportPackDraft: boolean;
  advancedMode: boolean;
  customSkillName: string;
  customSkillDescription: string;
  customWorkflowKind: "pack_builder_recipe" | "automation_v2_dag";
}

interface ProviderOption {
  id: string;
  models: string[];
}

interface McpServerOption {
  name: string;
  connected: boolean;
  enabled: boolean;
}

// ─── Constants ──────────────────────────────────────────────────────────────

const SCHEDULE_PRESETS: SchedulePreset[] = [
  {
    label: "Every hour",
    desc: "Good for monitoring & alerts",
    icon: "⏰",
    cron: "",
    intervalSeconds: 3600,
  },
  { label: "Every morning", desc: "Daily digest at 9 AM", icon: "☀️", cron: "0 9 * * *" },
  { label: "Every evening", desc: "End-of-day summary at 6 PM", icon: "🌙", cron: "0 18 * * *" },
  { label: "Daily at midnight", desc: "Nightly data processing", icon: "🔄", cron: "0 0 * * *" },
  { label: "Weekly Monday", desc: "Weekly reports & reviews", icon: "📋", cron: "0 9 * * 1" },
  { label: "Manual only", desc: "Run whenever you want", icon: "🎯", cron: "" },
];

const EXECUTION_MODES: {
  id: ExecutionMode;
  label: string;
  icon: string;
  desc: string;
  bestFor: string;
}[] = [
  {
    id: "single",
    label: "Single Agent",
    icon: "🤖",
    desc: "One focused AI handles the whole task",
    bestFor: "Simple, well-defined tasks",
  },
  {
    id: "team",
    label: "Agent Team",
    icon: "👥",
    desc: "Multiple specialized AIs collaborate with a planner and workers",
    bestFor: "Complex multi-step tasks (recommended)",
  },
  {
    id: "swarm",
    label: "Swarm",
    icon: "🐝",
    desc: "A swarm of AIs work in parallel on sub-tasks",
    bestFor: "Large-scale parallel work",
  },
];

const GOAL_EXAMPLES = [
  "Check my email every morning and send me a summary of what's important",
  "Monitor GitHub issues and post daily standup updates to Slack",
  "Summarize my calendar each Sunday and plan the upcoming week",
  "Watch for price changes on competitor products and alert me",
  "Generate a weekly report from our Notion workspace",
];

const AUTOMATION_PLANNER_SEED_KEY = "tandem.automations.plannerSeed";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

function normalizeMcpServers(raw: any): McpServerOption[] {
  if (Array.isArray(raw?.servers)) {
    return raw.servers
      .map((row: any) => {
        const name = String(row?.name || "").trim();
        if (!name) return null;
        return {
          name,
          connected: !!row?.connected,
          enabled: row?.enabled !== false,
        };
      })
      .filter((row): row is McpServerOption => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }
  if (raw && typeof raw === "object") {
    return Object.entries(raw)
      .map(([name, row]) => {
        const cleanName = String(name || "").trim();
        if (!cleanName) return null;
        const cfg = row && typeof row === "object" ? row : {};
        return {
          name: cleanName,
          connected: !!(cfg as any).connected,
          enabled: (cfg as any).enabled !== false,
        };
      })
      .filter((row): row is McpServerOption => !!row)
      .sort((a, b) => a.name.localeCompare(b.name));
  }
  return [];
}

function toSchedulePayload(wizard: WizardState) {
  const customCron = String(wizard.cron || "").trim();
  if (customCron) {
    return { cron: { expression: customCron } };
  }
  const preset = SCHEDULE_PRESETS.find((p) => p.label === wizard.schedulePreset);
  if (preset?.intervalSeconds) {
    return { interval_seconds: { seconds: preset.intervalSeconds } };
  }
  if (preset?.cron) {
    return { cron: { expression: preset.cron } };
  }
  return { type: "manual" };
}

function formatScheduleLabel(schedule: any) {
  const cronExpr = String(schedule?.cron?.expression || schedule?.cron_expression || "").trim();
  if (cronExpr) return cronExpr;
  const seconds = Number(schedule?.interval_seconds?.seconds);
  if (Number.isFinite(seconds) && seconds > 0) {
    if (seconds % 3600 === 0) return `Every ${seconds / 3600}h`;
    if (seconds % 60 === 0) return `Every ${seconds / 60}m`;
    return `Every ${seconds}s`;
  }
  return "manual";
}

function formatAutomationV2ScheduleLabel(schedule: any) {
  const type = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  if (type === "cron")
    return String(schedule?.cron_expression || schedule?.cronExpression || "cron");
  if (type === "interval") {
    const seconds = Number(schedule?.interval_seconds || schedule?.intervalSeconds || 0);
    if (!Number.isFinite(seconds) || seconds <= 0) return "interval";
    if (seconds % 3600 === 0) return `Every ${seconds / 3600}h`;
    if (seconds % 60 === 0) return `Every ${seconds / 60}m`;
    return `Every ${seconds}s`;
  }
  return "manual";
}

function validateWorkspaceRootInput(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "Workspace root is required.";
  if (!value.startsWith("/")) return "Workspace root must be an absolute path.";
  return "";
}

function buildOperatorPreferences(wizard: WizardState) {
  let roleModels: Record<string, unknown> | undefined;
  const rawRoleModels = String(wizard.roleModelsJson || "").trim();
  if (rawRoleModels) {
    try {
      const parsed = JSON.parse(rawRoleModels);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        roleModels = parsed as Record<string, unknown>;
      }
    } catch {
      roleModels = undefined;
    }
  }
  const maxParallelAgents =
    wizard.mode === "swarm"
      ? Math.max(1, Math.min(16, Number.parseInt(String(wizard.maxAgents || "4"), 10) || 4))
      : 1;
  const payload: Record<string, unknown> = {
    execution_mode: wizard.mode,
    max_parallel_agents: maxParallelAgents,
  };
  if (String(wizard.modelProvider || "").trim()) {
    payload.model_provider = String(wizard.modelProvider).trim();
  }
  if (String(wizard.modelId || "").trim()) {
    payload.model_id = String(wizard.modelId).trim();
  }
  if (roleModels && Object.keys(roleModels).length) {
    payload.role_models = roleModels;
  }
  return payload;
}

function validateRoleModelsJsonInput(raw: string) {
  const value = String(raw || "").trim();
  if (!value) return "";
  try {
    const parsed = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return "Role model overrides must be a JSON object.";
    }
    return "";
  } catch {
    return "Role model overrides must be valid JSON.";
  }
}

function scheduleToEditor(schedule: any) {
  const cronExpression = String(
    schedule?.cron?.expression || schedule?.cron_expression || schedule?.cron || ""
  ).trim();
  const intervalValue = Number(
    schedule?.interval_seconds?.seconds || schedule?.interval_seconds || 3600
  );
  const intervalSeconds =
    Number.isFinite(intervalValue) && intervalValue > 0 ? Math.round(intervalValue) : 3600;
  return {
    scheduleKind: cronExpression ? ("cron" as const) : ("interval" as const),
    cronExpression,
    intervalSeconds,
  };
}

function isActiveRunStatus(status: string) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
  return [
    "queued",
    "running",
    "in_progress",
    "executing",
    "pending_approval",
    "awaiting_approval",
  ].includes(normalized);
}

function runTimeLabel(run: any) {
  const started = Number(run?.started_at_ms || run?.fired_at_ms || run?.created_at_ms || 0);
  if (!Number.isFinite(started) || started <= 0) return "time unavailable";
  const deltaMs = Date.now() - started;
  const seconds = Math.max(0, Math.floor(deltaMs / 1000));
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h`;
}

function deriveRunDebugHints(run: any, artifacts: any[]) {
  const hints: string[] = [];
  const status = String(run?.status || "")
    .trim()
    .toLowerCase();
  if (status === "pending_approval" || status === "awaiting_approval") {
    hints.push("Run is waiting for approval before external actions.");
  }
  if (status === "blocked_policy") {
    hints.push("Run was blocked by policy. Check tool allowlist and integration permissions.");
  }
  if (status === "failed" || status === "error") {
    hints.push("Run failed. Inspect detail/error fields for root cause.");
  }
  if ((status === "completed" || status === "done") && !artifacts.length) {
    hints.push("Run completed but produced no artifacts. Verify output target and tool actions.");
  }
  if (run?.requires_approval === true) {
    hints.push("Automation policy requires human approval. Disable it for fully automated runs.");
  }
  return hints;
}

function eventRunId(event: any) {
  const props = event?.properties || {};
  return String(
    event?.run_id ||
      event?.runId ||
      event?.runID ||
      props?.run_id ||
      props?.runId ||
      props?.runID ||
      props?.run?.id ||
      ""
  ).trim();
}

function eventType(event: any) {
  return String(event?.type || event?.event_type || event?.event || "").trim();
}

function eventAt(event: any) {
  const props = event?.properties || {};
  const raw =
    event?.timestamp_ms ||
    event?.timestampMs ||
    props?.timestamp_ms ||
    props?.timestampMs ||
    props?.firedAtMs ||
    Date.now();
  const value = Number(raw);
  return Number.isFinite(value) ? value : Date.now();
}

function normalizeTimestamp(raw: any) {
  const value = Number(raw || 0);
  if (!Number.isFinite(value) || value <= 0) return Date.now();
  return value < 1_000_000_000_000 ? value * 1000 : value;
}

function extractSessionIdsFromRun(run: any) {
  const direct = Array.isArray(run?.active_session_ids)
    ? run.active_session_ids
    : Array.isArray(run?.activeSessionIds)
      ? run.activeSessionIds
      : [];
  const latest = [
    String(run?.latest_session_id || "").trim(),
    String(run?.latestSessionId || "").trim(),
  ].filter(Boolean);
  return Array.from(
    new Set(
      [...latest, ...direct.map((row: any) => String(row || "").trim()).filter(Boolean)].filter(
        Boolean
      )
    )
  );
}

function sessionMessageText(message: any) {
  const parts = Array.isArray(message?.parts) ? message.parts : [];
  const rows = parts
    .map((part: any) => {
      const type = String(part?.type || "").trim();
      if (type === "text" || type === "reasoning") return String(part?.text || "").trim();
      if (type === "tool") {
        const tool = String(part?.tool || "tool").trim();
        const error = String(part?.error || "").trim();
        const result = part?.result ? formatJson(part.result) : "";
        return [`tool: ${tool}`, error ? `error: ${error}` : "", result].filter(Boolean).join("\n");
      }
      return String(part?.text || "").trim();
    })
    .filter(Boolean);
  return rows.join("\n\n").trim();
}

function sessionMessageVariant(message: any) {
  const role = String(message?.info?.role || "")
    .trim()
    .toLowerCase();
  if (role === "user") return "user";
  if (role === "assistant") return "assistant";
  const body = sessionMessageText(message).toLowerCase();
  if (body.includes("engine_error") || body.includes("error")) return "error";
  return "system";
}

function sessionMessageParts(message: any) {
  return Array.isArray(message?.parts) ? message.parts : [];
}

function eventReason(event: any) {
  const props = event?.properties || event || {};
  return String(
    props?.reason ||
      props?.detail ||
      props?.error?.message ||
      props?.error ||
      props?.message ||
      props?.status ||
      ""
  ).trim();
}

function buildRunBlockers(run: any, sessionEvents: any[], runEvents: any[]) {
  const blockers: Array<{
    key: string;
    title: string;
    reason: string;
    source: string;
    at?: number;
  }> = [];
  const push = (key: string, title: string, reason: string, source: string, at?: number) => {
    if (!reason.trim()) return;
    if (blockers.some((row) => row.key === key)) return;
    blockers.push({ key, title, reason, source, at });
  };

  if (run?.requires_approval === true || String(run?.status || "").trim() === "pending_approval") {
    push(
      "approval-required",
      "Approval required",
      String(
        run?.approval_reason || "Manual approval required before external side effects."
      ).trim(),
      "policy"
    );
  }
  if (String(run?.denial_reason || "").trim()) {
    push("denied", "Run denied", String(run.denial_reason).trim(), "run");
  }
  if (String(run?.paused_reason || "").trim()) {
    push("paused", "Run paused", String(run.paused_reason).trim(), "run");
  }
  if (String(run?.detail || "").trim()) {
    const detail = String(run.detail).trim();
    if (
      detail.toLowerCase().includes("tool") ||
      detail.toLowerCase().includes("permission") ||
      detail.toLowerCase().includes("approval") ||
      detail.toLowerCase().includes("mcp") ||
      detail.toLowerCase().includes("auth")
    ) {
      push("detail", "Run detail", detail, "run");
    }
  }
  if (!extractSessionIdsFromRun(run).length) {
    push(
      "missing-session",
      "No linked session transcript",
      "This run does not expose a linked session transcript, so only telemetry/history are available.",
      "run"
    );
  }

  [...sessionEvents, ...runEvents].forEach((row: any) => {
    const type = String(eventType(row?.event || row) || "").trim();
    const reason = eventReason(row?.event || row);
    const at = Number(row?.at || eventAt(row?.event || row) || 0);
    if (
      type === "permission.asked" ||
      type === "approval.required" ||
      type === "routine.approval_required"
    ) {
      push(`event-${type}`, "Permission or approval required", reason || type, "permission", at);
    }
    if (type === "mcp.auth.required") {
      push(
        `event-${type}`,
        "MCP auth required",
        reason || "An MCP connector requires authorization.",
        "mcp",
        at
      );
    }
    if (type === "session.error" || type === "run.failed" || type === "routine.run.failed") {
      push(`event-${type}`, "Execution failure", reason || type, "session", at);
    }
    if (reason.toLowerCase().includes("no further tool calls")) {
      push("tool-mode", "Tool policy blocked progress", reason, "policy", at);
    }
    if (reason.toLowerCase().includes("timed out")) {
      push(`timeout-${type || at}`, "Timeout", reason, "session", at);
    }
  });

  return blockers.sort((a, b) => (b.at || 0) - (a.at || 0));
}

// ─── Wizard Steps ───────────────────────────────────────────────────────────

function Step1Goal({
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
}: {
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
}) {
  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        Describe what you want the AI to do — in plain English. No technical knowledge needed.
      </p>
      <textarea
        className="tcp-input min-h-[120px] text-base"
        placeholder={`e.g. "${GOAL_EXAMPLES[0]}"`}
        value={value}
        onInput={(e) => onChange((e.target as HTMLTextAreaElement).value)}
        autoFocus
      />
      <div className="grid gap-2">
        <p className="text-xs text-slate-500">Need inspiration? Try one of these:</p>
        <div className="flex flex-wrap gap-2">
          {GOAL_EXAMPLES.slice(1).map((ex) => (
            <button
              key={ex}
              className="tcp-btn truncate text-left text-xs"
              style={{ maxWidth: "280px" }}
              onClick={() => onChange(ex)}
            >
              {ex}
            </button>
          ))}
        </div>
      </div>
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
            Advanced: Reusable Skill Export
          </span>
          <div className="flex items-center gap-2">
            <button className="tcp-btn h-7 px-2 text-xs" onClick={onToggleAdvancedMode}>
              {advancedMode ? "Hide Advanced" : "Show Advanced"}
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
          This does not power the default automation path. Use it only if you want to export the
          planned workflow into a reusable skill scaffold.
        </p>
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
            <p>
              Suggested skill:{" "}
              <strong>{String(generatedSkill?.router?.skill_name || "new optional skill")}</strong>
            </p>
            <p className="text-slate-400">
              Artifacts:{" "}
              {Object.keys((generatedSkill?.artifacts as Record<string, string>) || {}).join(
                ", "
              ) || "SKILL.md, workflow.yaml, automation.example.yaml"}
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
            Generate a reusable skill draft after planning if you want to save this workflow for
            later reuse.
          </p>
        )}
        {installStatus ? <p className="mt-2 text-slate-300">{installStatus}</p> : null}
      </div>
    </div>
  );
}

function Step2Schedule({
  selected,
  onSelect,
  customCron,
  onCustomCron,
}: {
  selected: string;
  onSelect: (preset: SchedulePreset) => void;
  customCron: string;
  onCustomCron: (v: string) => void;
}) {
  return (
    <div className="grid gap-3">
      <p className="text-sm text-slate-400">When should this automation run?</p>
      <div className="grid gap-2 sm:grid-cols-2">
        {SCHEDULE_PRESETS.map((preset) => (
          <button
            key={preset.label}
            onClick={() => onSelect(preset)}
            className={`tcp-list-item flex flex-col items-start gap-1 text-left transition-all ${
              selected === preset.label ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <div className="flex items-center gap-2 font-medium">
              <span>{preset.icon}</span>
              <span>{preset.label}</span>
            </div>
            <span className="tcp-subtle text-xs">{preset.desc}</span>
            {preset.cron ? (
              <code className="rounded bg-slate-800/60 px-1.5 py-0.5 text-xs text-slate-400">
                {preset.cron}
              </code>
            ) : null}
          </button>
        ))}
      </div>
      <div className="grid gap-1">
        <label className="text-xs text-slate-500">Custom cron expression (advanced)</label>
        <input
          className="tcp-input font-mono text-sm"
          placeholder="e.g. 30 8 * * 1-5  (8:30am weekdays)"
          value={customCron}
          onInput={(e) => onCustomCron((e.target as HTMLInputElement).value)}
        />
      </div>
    </div>
  );
}

function Step3Mode({
  selected,
  onSelect,
  maxAgents,
  onMaxAgents,
  workspaceRoot,
  onWorkspaceRootChange,
  providerOptions,
  providerId,
  modelId,
  onProviderChange,
  onModelChange,
  roleModelsJson,
  onRoleModelsChange,
  roleModelsError,
  mcpServers,
  selectedMcpServers,
  onToggleMcpServer,
  onOpenMcpSettings,
  workspaceRootError,
}: {
  selected: ExecutionMode;
  onSelect: (mode: ExecutionMode) => void;
  maxAgents: string;
  onMaxAgents: (v: string) => void;
  workspaceRoot: string;
  onWorkspaceRootChange: (v: string) => void;
  providerOptions: ProviderOption[];
  providerId: string;
  modelId: string;
  onProviderChange: (v: string) => void;
  onModelChange: (v: string) => void;
  roleModelsJson: string;
  onRoleModelsChange: (v: string) => void;
  roleModelsError: string;
  mcpServers: McpServerOption[];
  selectedMcpServers: string[];
  onToggleMcpServer: (name: string) => void;
  onOpenMcpSettings: () => void;
  workspaceRootError: string;
}) {
  const modelOptions = providerOptions.find((p) => p.id === providerId)?.models || [];
  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        How should the AI handle this task? (You can always change this later.)
      </p>
      <div className="grid gap-3">
        {EXECUTION_MODES.map((m) => (
          <button
            key={m.id}
            onClick={() => onSelect(m.id)}
            className={`tcp-list-item flex items-start gap-4 text-left transition-all ${
              selected === m.id ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <span className="mt-0.5 text-2xl">{m.icon}</span>
            <div className="grid gap-1">
              <div className="flex items-center gap-2">
                <span className="font-semibold">{m.label}</span>
                {m.id === "team" ? (
                  <span className="rounded-full bg-amber-500/20 px-2 py-0.5 text-xs text-amber-300">
                    Recommended
                  </span>
                ) : null}
              </div>
              <span className="text-sm text-slate-300">{m.desc}</span>
              <span className="tcp-subtle text-xs">Best for: {m.bestFor}</span>
            </div>
            <div
              className="ml-auto mt-1 h-4 w-4 shrink-0 rounded-full border-2 border-slate-600 transition-all data-[checked]:border-amber-400 data-[checked]:bg-amber-400/30"
              data-checked={selected === m.id ? true : undefined}
            />
          </button>
        ))}
      </div>
      {selected === "swarm" ? (
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Max parallel agents</label>
          <input
            type="number"
            min="2"
            max="16"
            className="tcp-input w-24"
            value={maxAgents}
            onInput={(e) => onMaxAgents((e.target as HTMLInputElement).value)}
          />
        </div>
      ) : null}
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Execution Directory</div>
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Workspace root</label>
          <input
            className={`tcp-input text-sm ${workspaceRootError ? "border-red-500/70 text-red-100" : ""}`}
            value={workspaceRoot}
            onInput={(e) => onWorkspaceRootChange((e.target as HTMLInputElement).value)}
            placeholder="/absolute/path/to/project"
          />
          <div className="text-xs text-slate-500">
            Tandem will run this automation from this workspace directory.
          </div>
          {workspaceRootError ? (
            <div className="text-xs text-red-300">{workspaceRootError}</div>
          ) : null}
        </div>
      </div>
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Model Selection</div>
        <div className="grid gap-2 sm:grid-cols-2">
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Provider</label>
            <select
              className="tcp-input text-sm"
              value={providerId}
              onInput={(e) => onProviderChange((e.target as HTMLSelectElement).value)}
            >
              <option value="">Use workspace default</option>
              {providerOptions.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.id}
                </option>
              ))}
            </select>
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Model</label>
            <input
              className="tcp-input text-sm"
              value={modelId}
              list={providerId ? `models-${providerId}` : undefined}
              onInput={(e) => onModelChange((e.target as HTMLInputElement).value)}
              placeholder="Use workspace default model"
            />
            {providerId ? (
              <datalist id={`models-${providerId}`}>
                {modelOptions.map((model) => (
                  <option key={model} value={model} />
                ))}
              </datalist>
            ) : null}
          </div>
        </div>
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Role model overrides (advanced JSON)</label>
          <textarea
            className={`tcp-input min-h-[72px] font-mono text-xs ${roleModelsError ? "border-red-500/70 text-red-100" : ""}`}
            value={roleModelsJson}
            onInput={(e) => onRoleModelsChange((e.target as HTMLTextAreaElement).value)}
            placeholder={`{"planner":{"provider_id":"openai","model_id":"gpt-5"},"worker":{"provider_id":"anthropic","model_id":"claude-sonnet-4"}}`}
          />
          {roleModelsError ? <div className="text-xs text-red-300">{roleModelsError}</div> : null}
        </div>
      </div>
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="flex items-center justify-between gap-2">
          <div className="text-xs uppercase tracking-wide text-slate-500">MCP Servers</div>
          <button className="tcp-btn h-7 px-2 text-xs" onClick={onOpenMcpSettings}>
            Add MCP Server
          </button>
        </div>
        {mcpServers.length ? (
          <div className="flex flex-wrap gap-2">
            {mcpServers.map((server) => {
              const isSelected = selectedMcpServers.includes(server.name);
              return (
                <button
                  key={server.name}
                  className={`tcp-btn h-7 px-2 text-xs ${isSelected ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                  onClick={() => onToggleMcpServer(server.name)}
                >
                  {server.name} {server.connected ? "• connected" : "• disconnected"}
                </button>
              );
            })}
          </div>
        ) : (
          <div className="text-xs text-slate-400">
            No MCP servers configured yet. Add one to allow external tools in this automation.
          </div>
        )}
      </div>
    </div>
  );
}

function Step4Review({
  wizard,
  onToggleExportPackDraft,
  onSubmit,
  isPending,
  planPreview,
  isPreviewing,
  planningConversation,
  planningChangeSummary,
  onSendPlanningMessage,
  isSendingPlanningMessage,
  onResetPlanningChat,
  isResettingPlanningChat,
  plannerError,
}: {
  wizard: WizardState;
  onToggleExportPackDraft: () => void;
  onSubmit: () => void;
  isPending: boolean;
  planPreview: any;
  isPreviewing: boolean;
  planningConversation: any;
  planningChangeSummary: string[];
  onSendPlanningMessage: (message: string) => void;
  isSendingPlanningMessage: boolean;
  onResetPlanningChat: () => void;
  isResettingPlanningChat: boolean;
  plannerError: string;
}) {
  const [planningNote, setPlanningNote] = useState("");
  const wizardSchedule = wizard.cron
    ? wizard.cron
    : SCHEDULE_PRESETS.find((p) => p.label === wizard.schedulePreset)?.intervalSeconds
      ? `Every ${SCHEDULE_PRESETS.find((p) => p.label === wizard.schedulePreset)!.intervalSeconds! / 3600}h`
      : wizard.schedulePreset || "Manual";
  const planOperatorPreferences =
    planPreview && typeof planPreview === "object"
      ? planPreview.operator_preferences || planPreview.operatorPreferences || {}
      : {};
  const effectiveMode = String(
    (planOperatorPreferences as any)?.execution_mode || wizard.mode || "team"
  ).trim() as ExecutionMode;
  const modeInfo = EXECUTION_MODES.find((m) => m.id === effectiveMode);
  const effectiveMaxParallel = Number(
    (planOperatorPreferences as any)?.max_parallel_agents ??
      (planOperatorPreferences as any)?.maxParallelAgents ??
      (effectiveMode === "swarm" ? wizard.maxAgents : 1)
  );
  const hasPlanPreview = !!planPreview;
  const effectiveModelProvider = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.model_provider ||
          (planOperatorPreferences as any)?.modelProvider ||
          ""
      : wizard.modelProvider || ""
  ).trim();
  const effectiveModelId = String(
    hasPlanPreview
      ? (planOperatorPreferences as any)?.model_id ||
          (planOperatorPreferences as any)?.modelId ||
          ""
      : wizard.modelId || ""
  ).trim();
  const effectiveWorkspaceRoot = String(
    planPreview?.workspace_root || planPreview?.workspaceRoot || wizard.workspaceRoot || ""
  ).trim();
  const effectiveMcpServers = Array.isArray(
    planPreview?.allowed_mcp_servers || planPreview?.allowedMcpServers
  )
    ? ((planPreview?.allowed_mcp_servers || planPreview?.allowedMcpServers || []) as string[])
    : wizard.selectedMcpServers;
  const effectiveSchedule = planPreview?.schedule
    ? formatAutomationV2ScheduleLabel(planPreview.schedule)
    : wizardSchedule;
  const effectivePlanTitle = String(planPreview?.title || "").trim();

  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">Review your automation before deploying.</p>

      {/* Summary card */}
      <div className="rounded-xl border border-slate-700/60 bg-slate-900/40 p-4 grid gap-3">
        {effectivePlanTitle ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Plan Title</span>
            <span className="text-sm font-semibold text-slate-100">{effectivePlanTitle}</span>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Goal</span>
          <span className="text-sm text-slate-100 italic">"{wizard.goal}"</span>
        </div>
        <div className="grid grid-cols-2 gap-3">
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Schedule</span>
            <span className="text-sm font-medium text-slate-200">{effectiveSchedule}</span>
          </div>
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Execution Mode</span>
            <span className="text-sm font-medium text-slate-200">
              {modeInfo?.icon} {modeInfo?.label || effectiveMode}
              {Number.isFinite(effectiveMaxParallel) && effectiveMaxParallel > 1
                ? ` · ${effectiveMaxParallel} agents`
                : ""}
            </span>
          </div>
        </div>
        {hasPlanPreview || effectiveModelProvider || effectiveModelId ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Model Override</span>
            <span className="text-sm font-medium text-slate-200">
              {effectiveModelProvider || effectiveModelId
                ? `${effectiveModelProvider || "default provider"} / ${effectiveModelId || "default model"}`
                : "Workspace default"}
            </span>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Workspace Root</span>
          <code className="rounded bg-slate-800/60 px-2 py-1 text-xs text-slate-300">
            {effectiveWorkspaceRoot || "engine workspace root"}
          </code>
        </div>
        {hasPlanPreview || effectiveMcpServers.length ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">MCP Servers</span>
            {effectiveMcpServers.length ? (
              <div className="flex flex-wrap gap-1">
                {effectiveMcpServers.map((name) => (
                  <span key={name} className="tcp-badge-info">
                    {name}
                  </span>
                ))}
              </div>
            ) : (
              <span className="text-sm font-medium text-slate-400">None</span>
            )}
          </div>
        ) : null}
        {wizard.routedSkill ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">
              Reusable Flow Match
            </span>
            <span className="text-sm font-medium text-slate-200">
              {wizard.routedSkill}
              {wizard.routingConfidence ? ` (${wizard.routingConfidence})` : ""}
            </span>
          </div>
        ) : null}
        {wizard.cron ? (
          <div className="grid gap-1">
            <span className="text-xs text-slate-500 uppercase tracking-wide">Cron</span>
            <code className="rounded bg-slate-800/60 px-2 py-1 text-xs text-slate-300">
              {wizard.cron}
            </code>
          </div>
        ) : null}
        <div className="grid gap-1">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Workflow Plan</span>
          {isPreviewing ? (
            <span className="text-sm text-slate-300">Planning workflow…</span>
          ) : planPreview ? (
            <div className="grid gap-1 text-sm text-slate-300">
              <span>
                Confidence: <strong>{String(planPreview?.confidence || "unknown")}</strong>
              </span>
              <span>
                Execution target:{" "}
                <strong>{String(planPreview?.execution_target || "automation_v2")}</strong>
              </span>
              {effectivePlanTitle ? (
                <span>
                  Title: <strong>{effectivePlanTitle}</strong>
                </span>
              ) : null}
              <span>
                Steps:{" "}
                <strong>{Array.isArray(planPreview?.steps) ? planPreview.steps.length : 0}</strong>
              </span>
              {Array.isArray(planPreview?.steps) && planPreview.steps.length ? (
                <div className="mt-1 grid gap-1">
                  {planPreview.steps.map((step: any, index: number) => (
                    <div
                      key={`${String(step?.step_id || step?.stepId || index)}-${index}`}
                      className="rounded-lg border border-slate-800 bg-slate-950/40 px-2 py-1"
                    >
                      <div className="text-xs font-medium text-slate-200">
                        {String(step?.step_id || step?.stepId || `step-${index + 1}`)}
                        {step?.kind ? (
                          <span className="ml-2 text-[11px] uppercase tracking-wide text-slate-500">
                            {String(step.kind)}
                          </span>
                        ) : null}
                      </div>
                      {typeof step?.objective === "string" && step.objective.trim() ? (
                        <div className="text-xs text-slate-400">{step.objective}</div>
                      ) : null}
                    </div>
                  ))}
                </div>
              ) : null}
              {typeof planPreview?.description === "string" && planPreview.description.trim() ? (
                <span>{planPreview.description}</span>
              ) : null}
            </div>
          ) : (
            <span className="text-sm text-slate-400">No workflow plan available yet.</span>
          )}
        </div>
      </div>

      {plannerError ? (
        <div className="rounded-xl border border-red-500/40 bg-red-950/30 p-3 text-sm text-red-200">
          {plannerError}
        </div>
      ) : null}

      {planningChangeSummary.length ? (
        <div className="rounded-xl border border-emerald-500/30 bg-emerald-950/20 p-3">
          <div className="text-xs uppercase tracking-wide text-emerald-300">
            Latest Plan Changes
          </div>
          <div className="mt-2 flex flex-wrap gap-2">
            {planningChangeSummary.map((item, index) => (
              <span key={`${item}-${index}`} className="tcp-badge-ok">
                {item}
              </span>
            ))}
          </div>
        </div>
      ) : null}

      <div className="rounded-xl border border-slate-700/60 bg-slate-900/40 p-4 grid gap-3">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-slate-500 uppercase tracking-wide">Planning Chat</span>
          <button
            className="tcp-btn h-7 px-2 text-xs"
            disabled={isResettingPlanningChat || !planPreview?.plan_id}
            onClick={onResetPlanningChat}
          >
            {isResettingPlanningChat ? "Resetting…" : "Reset Plan"}
          </button>
        </div>
        <div className="rounded-lg border border-amber-500/30 bg-amber-950/20 px-3 py-2 text-xs text-amber-200">
          Planning chat is currently limited to deterministic edits like schedule, workspace root,
          title, MCP servers, execution mode, and model overrides. Broader workflow rewrites are not
          supported in this slice yet.
        </div>
        <div className="max-h-56 overflow-auto rounded-lg border border-slate-800 bg-slate-950/50 p-3">
          {Array.isArray(planningConversation?.messages) && planningConversation.messages.length ? (
            <div className="grid gap-3">
              {planningConversation.messages.map((message: any, index: number) => (
                <div key={`${message?.created_at_ms || index}-${index}`} className="grid gap-1">
                  <span className="text-[11px] uppercase tracking-wide text-slate-500">
                    {String(message?.role || "assistant")}
                  </span>
                  <div className="text-sm text-slate-200">{String(message?.text || "").trim()}</div>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-sm text-slate-400">
              Add planning notes here to revise the workflow before creating it.
            </div>
          )}
        </div>
        <textarea
          className="tcp-input min-h-[84px] text-sm"
          value={planningNote}
          onInput={(e) => setPlanningNote((e.target as HTMLTextAreaElement).value)}
          placeholder='Example: "Make this weekly, run it from /srv/acme/app, and remove notifications."'
        />
        <div className="flex justify-end">
          <button
            className="tcp-btn-primary"
            disabled={isSendingPlanningMessage || !planningNote.trim() || !planPreview?.plan_id}
            onClick={() => {
              const note = planningNote.trim();
              if (!note) return;
              onSendPlanningMessage(note);
              setPlanningNote("");
            }}
          >
            {isSendingPlanningMessage ? "Updating plan…" : "Update Plan"}
          </button>
        </div>
      </div>

      <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
        <label className="flex items-start gap-3 rounded-lg border border-slate-700/50 bg-slate-900/30 p-3 text-sm text-slate-300">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={wizard.exportPackDraft}
            onChange={onToggleExportPackDraft}
          />
          <span className="grid gap-1">
            <span className="font-medium text-slate-200">Also export a reusable pack draft</span>
            <span className="text-xs text-slate-400">
              After creating the automation, Tandem will also create a Pack Builder draft so this
              workflow can be saved and reused later.
            </span>
          </span>
        </label>
      </div>

      <div className="rounded-xl border border-slate-700/40 bg-slate-800/20 p-3 text-xs text-slate-400">
        💡 Tandem will save this automation and schedule a{" "}
        <strong className="text-slate-300">{modeInfo?.label || effectiveMode}</strong> that runs{" "}
        <strong className="text-slate-300">{effectiveSchedule}</strong>. You can pause, edit or
        delete it anytime.
      </div>

      <button
        className="tcp-btn-primary"
        disabled={isPending || isPreviewing || !wizard.goal.trim() || !planPreview}
        onClick={onSubmit}
      >
        {isPending ? "Creating automation…" : "🚀 Create Automation"}
      </button>
    </div>
  );
}

// ─── Wizard Container ───────────────────────────────────────────────────────

function CreateWizard({
  client,
  toast,
  navigate,
  defaultProvider,
  defaultModel,
}: {
  client: any;
  toast: any;
  navigate: (route: string) => void;
  defaultProvider: string;
  defaultModel: string;
}) {
  const queryClient = useQueryClient();
  const [step, setStep] = useState<WizardStep>(1);
  const [planSource, setPlanSource] = useState<string>("automations_page");
  const [routerMatches, setRouterMatches] = useState<
    Array<{ skill_name?: string; confidence?: number }>
  >([]);
  const [planPreview, setPlanPreview] = useState<any>(null);
  const [planningConversation, setPlanningConversation] = useState<any>(null);
  const [planningChangeSummary, setPlanningChangeSummary] = useState<string[]>([]);
  const [plannerError, setPlannerError] = useState<string>("");
  const [validationBadge, setValidationBadge] = useState<string>("");
  const [generatedSkill, setGeneratedSkill] = useState<any>(null);
  const [showArtifactPreview, setShowArtifactPreview] = useState<boolean>(false);
  const [artifactPreviewKey, setArtifactPreviewKey] = useState<string>("SKILL.md");
  const [installStatus, setInstallStatus] = useState<string>("");
  const [wizard, setWizard] = useState<WizardState>({
    goal: "",
    workspaceRoot: "",
    schedulePreset: "Every morning",
    cron: "",
    mode: "team",
    maxAgents: "4",
    routedSkill: "",
    routingConfidence: "",
    modelProvider: String(defaultProvider || ""),
    modelId: String(defaultModel || ""),
    roleModelsJson: "",
    selectedMcpServers: [],
    exportPackDraft: false,
    advancedMode: false,
    customSkillName: "",
    customSkillDescription: "",
    customWorkflowKind: "pack_builder_recipe",
  });

  const providersCatalogQuery = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [] })),
    refetchInterval: 30000,
  });

  const providersConfigQuery = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({})),
    refetchInterval: 30000,
  });

  const mcpServersQuery = useQuery({
    queryKey: ["mcp", "servers"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 12000,
  });

  const healthQuery = useQuery({
    queryKey: ["global", "health"],
    queryFn: () => client.health().catch(() => ({})),
    refetchInterval: 30000,
  });

  const providerOptions = useMemo(() => {
    const rows = Array.isArray(providersCatalogQuery.data?.all)
      ? providersCatalogQuery.data.all
      : [];
    return rows
      .map((provider: any) => ({
        id: String(provider?.id || "").trim(),
        models: Object.keys(provider?.models || {}),
      }))
      .filter((provider: ProviderOption) => !!provider.id)
      .sort((a: ProviderOption, b: ProviderOption) => a.id.localeCompare(b.id));
  }, [providersCatalogQuery.data]);

  const mcpServers = useMemo(
    () => normalizeMcpServers(mcpServersQuery.data),
    [mcpServersQuery.data]
  );
  useEffect(() => {
    const configDefaultProvider = String(
      providersConfigQuery.data?.default || defaultProvider || ""
    ).trim();
    if (!configDefaultProvider) return;
    const models =
      providerOptions.find((provider) => provider.id === configDefaultProvider)?.models || [];
    const configDefaultModel = String(
      providersConfigQuery.data?.providers?.[configDefaultProvider]?.default_model ||
        defaultModel ||
        models[0] ||
        ""
    ).trim();
    setWizard((current) => {
      if (current.modelProvider && current.modelId) return current;
      return {
        ...current,
        modelProvider: current.modelProvider || configDefaultProvider,
        modelId: current.modelId || configDefaultModel,
      };
    });
  }, [defaultModel, defaultProvider, providerOptions, providersConfigQuery.data]);

  useEffect(() => {
    const defaultWorkspaceRoot = String(
      (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
    ).trim();
    if (!defaultWorkspaceRoot) return;
    setWizard((current) => {
      if (String(current.workspaceRoot || "").trim()) return current;
      return {
        ...current,
        workspaceRoot: defaultWorkspaceRoot,
      };
    });
  }, [healthQuery.data]);

  const matchMutation = useMutation({
    mutationFn: async (goal: string) => {
      if (!goal.trim() || !client?.skills?.match) {
        return null;
      }
      return client.skills.match({ goal, maxMatches: 3, threshold: 0.35 });
    },
    onError: () => {
      // Keep routing non-blocking.
    },
  });

  const compileMutation = useMutation({
    mutationFn: async () => {
      if (!client?.workflowPlans?.chatStart) {
        return null;
      }
      const response = await client.workflowPlans.chatStart({
        prompt: wizard.goal,
        schedule: toSchedulePayload(wizard),
        plan_source: planSource,
        allowed_mcp_servers: wizard.selectedMcpServers,
        workspace_root: wizard.workspaceRoot,
        operator_preferences: buildOperatorPreferences(wizard),
      });
      return response || null;
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerError("");
    },
    onError: (error) => {
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlannerError(error instanceof Error ? error.message : String(error));
    },
  });

  const planningMessageMutation = useMutation({
    mutationFn: async (message: string) => {
      if (!client?.workflowPlans?.chatMessage || !planPreview?.plan_id) {
        return null;
      }
      return client.workflowPlans.chatMessage({
        plan_id: planPreview.plan_id,
        message,
      });
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary(
        Array.isArray(res?.change_summary)
          ? res.change_summary.map((row: any) => String(row || "").trim()).filter(Boolean)
          : []
      );
      setPlannerError(
        typeof res?.clarifier?.question === "string" ? String(res.clarifier.question) : ""
      );
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      toast("err", message);
    },
  });

  const planningResetMutation = useMutation({
    mutationFn: async () => {
      if (!client?.workflowPlans?.chatReset || !planPreview?.plan_id) {
        return null;
      }
      return client.workflowPlans.chatReset({
        plan_id: planPreview.plan_id,
      });
    },
    onSuccess: (res) => {
      setPlanPreview(res?.plan || null);
      setPlanningConversation(res?.conversation || null);
      setPlanningChangeSummary([]);
      setPlannerError("");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      toast("err", message);
    },
  });

  const validateSkillMutation = useMutation({
    mutationFn: async (skillName: string) => {
      if (!client?.skills?.get || !client?.skills?.validate) {
        return null;
      }
      const loaded = await client.skills.get(skillName);
      const content = (loaded as any)?.content;
      if (!content) {
        return null;
      }
      return client.skills.validate({ content });
    },
    onSuccess: (res) => {
      if (!res) {
        setValidationBadge("");
        return;
      }
      setValidationBadge(res.invalid > 0 ? "not_validated" : "validated");
    },
    onError: () => setValidationBadge("not_validated"),
  });

  const generateSkillMutation = useMutation({
    mutationFn: async () => {
      if (!client?.skills?.generate || !wizard.goal.trim()) {
        return null;
      }
      const prompt = wizard.advancedMode
        ? [
            wizard.goal.trim(),
            wizard.customSkillName ? `Skill name: ${wizard.customSkillName}` : "",
            wizard.customSkillDescription ? `Description: ${wizard.customSkillDescription}` : "",
            `Workflow kind: ${wizard.customWorkflowKind}`,
          ]
            .filter(Boolean)
            .join("\n")
        : wizard.goal;
      return client.skills.generate({ prompt });
    },
    onSuccess: (res) => {
      setGeneratedSkill(res);
      const firstKey = Object.keys((res as any)?.artifacts || {})[0];
      setArtifactPreviewKey(firstKey || "SKILL.md");
      setShowArtifactPreview(false);
      setInstallStatus("");
    },
    onError: () => {
      setGeneratedSkill(null);
      setShowArtifactPreview(false);
      setInstallStatus("Optional skill generation failed.");
    },
  });

  const installGeneratedSkillMutation = useMutation({
    mutationFn: async () => {
      if (!client?.skills?.generateInstall) {
        return null;
      }
      const artifacts = generatedSkill?.artifacts as Record<string, string> | undefined;
      if (!artifacts || !artifacts["SKILL.md"]) {
        throw new Error("No generated artifacts available to install.");
      }
      return client.skills.generateInstall({
        location: "project",
        conflictPolicy: "rename",
        artifacts: {
          "SKILL.md": artifacts["SKILL.md"],
          "workflow.yaml": artifacts["workflow.yaml"],
          "automation.example.yaml": artifacts["automation.example.yaml"],
        },
      });
    },
    onSuccess: (res) => {
      const name = (res as any)?.skill?.name;
      setInstallStatus(
        name
          ? `Installed optional skill as '${String(name)}' in project skills.`
          : "Installed optional skill in project skills."
      );
      void queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) =>
      setInstallStatus(`Install failed: ${error instanceof Error ? error.message : String(error)}`),
  });

  const deployMutation = useMutation({
    mutationFn: async () => {
      if (!wizard.goal.trim()) throw new Error("Please describe your goal first.");
      const preview =
        planPreview ||
        (await compileMutation.mutateAsync().catch((error: unknown) => {
          throw error instanceof Error ? error : new Error(String(error));
        }));
      const nextPlan = preview?.plan || preview;
      if (!nextPlan) {
        throw new Error("Workflow plan preview failed.");
      }
      return client.workflowPlans.apply({
        plan: nextPlan,
        creator_id: "control-panel",
        ...(wizard.exportPackDraft
          ? {
              pack_builder_export: {
                enabled: true,
                auto_apply: false,
              },
            }
          : {}),
      });
    },
    onSuccess: async (res) => {
      const exportStatus = res?.pack_builder_export?.status;
      if (exportStatus === "preview_pending") {
        toast(
          "ok",
          "🎉 Automation created and reusable pack draft exported. Check Pack Builder to continue."
        );
      } else {
        toast("ok", "🎉 Automation created! Check 'My Automations' to see it running.");
      }
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["mcp"] }),
      ]);
      setWizard({
        goal: "",
        workspaceRoot: String(
          (healthQuery.data as any)?.workspaceRoot ||
            (healthQuery.data as any)?.workspace_root ||
            ""
        ).trim(),
        schedulePreset: "Every morning",
        cron: "",
        mode: "team",
        maxAgents: "4",
        routedSkill: "",
        routingConfidence: "",
        modelProvider: String(defaultProvider || ""),
        modelId: String(defaultModel || ""),
        roleModelsJson: "",
        selectedMcpServers: [],
        exportPackDraft: false,
        advancedMode: false,
        customSkillName: "",
        customSkillDescription: "",
        customWorkflowKind: "pack_builder_recipe",
      });
      setRouterMatches([]);
      setPlanSource("automations_page");
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      setPlannerError("");
      setValidationBadge("");
      setGeneratedSkill(null);
      setShowArtifactPreview(false);
      setArtifactPreviewKey("SKILL.md");
      setInstallStatus("");
      setStep(1);
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setPlannerError(message);
      toast("err", message);
    },
  });

  const workspaceRootError = validateWorkspaceRootInput(wizard.workspaceRoot);
  const roleModelsError = validateRoleModelsJsonInput(wizard.roleModelsJson);

  const canAdvance =
    step === 1
      ? wizard.goal.trim().length > 8
      : step === 2
        ? !!wizard.schedulePreset || !!wizard.cron.trim()
        : step === 3
          ? !!wizard.mode && !workspaceRootError && !roleModelsError
          : true;

  const STEPS = ["What?", "When?", "How?", "Review"];
  const goToNextStep = async () => {
    if (step === 1) {
      const result = await matchMutation.mutateAsync(wizard.goal);
      if (result && result.decision === "match" && result.skill_name) {
        void validateSkillMutation.mutateAsync(String(result.skill_name));
        setWizard((s) => ({
          ...s,
          routedSkill: String(result.skill_name),
          routingConfidence:
            typeof result.confidence === "number" ? `${Math.round(result.confidence * 100)}%` : "",
        }));
      } else {
        setValidationBadge("");
        setWizard((s) => ({
          ...s,
          routedSkill: "",
          routingConfidence: "",
        }));
      }
      const top = Array.isArray((result as any)?.top_matches) ? (result as any).top_matches : [];
      setRouterMatches(top);
    }
    const next = (step + 1) as WizardStep;
    setStep(next);
    if (next === 4) {
      setPlannerError("");
      setPlanPreview(null);
      setPlanningConversation(null);
      setPlanningChangeSummary([]);
      void compileMutation.mutateAsync();
    }
  };

  useEffect(() => {
    if (step !== 1) return;
    try {
      const raw = sessionStorage.getItem(AUTOMATION_PLANNER_SEED_KEY);
      if (!raw) return;
      sessionStorage.removeItem(AUTOMATION_PLANNER_SEED_KEY);
      const seed = JSON.parse(raw);
      const prompt = String(seed?.prompt || "").trim();
      if (!prompt) return;
      const nextPlanSource = String(seed?.plan_source || "chat_setup").trim() || "chat_setup";
      setPlanSource(nextPlanSource);
      setWizard((current) => ({
        ...current,
        goal: prompt,
      }));
    } catch {
      // ignore
    }
  }, [step]);

  return (
    <div className="grid gap-4">
      {/* Progress Bar */}
      <div className="flex items-center gap-2">
        {STEPS.map((label, i) => {
          const num = (i + 1) as WizardStep;
          const active = num === step;
          const done = num < step;
          return (
            <div key={label} className="flex-1">
              <button
                className={`mb-1 flex w-full items-center gap-1.5 rounded-lg px-2 py-1 text-xs font-medium transition-all ${
                  active
                    ? "bg-amber-500/20 text-amber-300"
                    : done
                      ? "text-slate-400"
                      : "text-slate-600"
                }`}
                onClick={() => done && setStep(num)}
              >
                <span
                  className={`flex h-5 w-5 items-center justify-center rounded-full text-xs font-bold ${
                    active
                      ? "bg-amber-500 text-black"
                      : done
                        ? "bg-slate-600 text-white"
                        : "bg-slate-800 text-slate-500"
                  }`}
                >
                  {done ? "✓" : num}
                </span>
                {label}
              </button>
              {/* Progress line */}
              <div className="h-0.5 w-full rounded-full bg-slate-800">
                <div
                  className="h-full rounded-full bg-amber-500 transition-all"
                  style={{ width: done ? "100%" : active ? "50%" : "0%" }}
                />
              </div>
            </div>
          );
        })}
      </div>

      {/* Step content */}
      <AnimatePresence mode="wait">
        <motion.div
          key={step}
          initial={{ opacity: 0, x: 16 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -16 }}
          transition={{ duration: 0.18 }}
        >
          {step === 1 ? (
            <Step1Goal
              value={wizard.goal}
              onChange={(v) => setWizard((s) => ({ ...s, goal: v }))}
              routedSkill={wizard.routedSkill}
              routingConfidence={wizard.routingConfidence}
              validationBadge={validationBadge}
              generatedSkill={generatedSkill}
              advancedMode={wizard.advancedMode}
              customSkillName={wizard.customSkillName}
              customSkillDescription={wizard.customSkillDescription}
              customWorkflowKind={wizard.customWorkflowKind}
              onToggleAdvancedMode={() =>
                setWizard((s) => ({ ...s, advancedMode: !s.advancedMode }))
              }
              onChangeCustomSkillName={(v) => setWizard((s) => ({ ...s, customSkillName: v }))}
              onChangeCustomSkillDescription={(v) =>
                setWizard((s) => ({ ...s, customSkillDescription: v }))
              }
              onChangeCustomWorkflowKind={(v) =>
                setWizard((s) => ({ ...s, customWorkflowKind: v }))
              }
              showArtifactPreview={showArtifactPreview}
              onToggleArtifactPreview={() => setShowArtifactPreview((v) => !v)}
              artifactPreviewKey={artifactPreviewKey}
              onSelectArtifactPreviewKey={(v) => setArtifactPreviewKey(v)}
              onGenerateSkill={() => {
                void generateSkillMutation.mutateAsync();
              }}
              onInstallGeneratedSkill={() => {
                void installGeneratedSkillMutation.mutateAsync();
              }}
              isGeneratingSkill={generateSkillMutation.isPending}
              isInstallingSkill={installGeneratedSkillMutation.isPending}
              installStatus={installStatus}
              topMatches={routerMatches}
              isMatching={matchMutation.isPending}
            />
          ) : step === 2 ? (
            <Step2Schedule
              selected={wizard.schedulePreset}
              onSelect={(preset) =>
                setWizard((s) => ({
                  ...s,
                  schedulePreset: preset.label,
                  cron: preset.cron,
                }))
              }
              customCron={wizard.cron}
              onCustomCron={(v) => setWizard((s) => ({ ...s, cron: v, schedulePreset: "" }))}
            />
          ) : step === 3 ? (
            <Step3Mode
              selected={wizard.mode}
              onSelect={(mode) => setWizard((s) => ({ ...s, mode }))}
              maxAgents={wizard.maxAgents}
              onMaxAgents={(v) => setWizard((s) => ({ ...s, maxAgents: v }))}
              workspaceRoot={wizard.workspaceRoot}
              onWorkspaceRootChange={(v) => setWizard((s) => ({ ...s, workspaceRoot: v }))}
              providerOptions={providerOptions}
              providerId={wizard.modelProvider}
              modelId={wizard.modelId}
              onProviderChange={(v) =>
                setWizard((s) => ({
                  ...s,
                  modelProvider: v,
                  modelId: v === s.modelProvider ? s.modelId : "",
                }))
              }
              onModelChange={(v) => setWizard((s) => ({ ...s, modelId: v }))}
              roleModelsJson={wizard.roleModelsJson}
              onRoleModelsChange={(v) => setWizard((s) => ({ ...s, roleModelsJson: v }))}
              roleModelsError={roleModelsError}
              mcpServers={mcpServers}
              selectedMcpServers={wizard.selectedMcpServers}
              onToggleMcpServer={(name) =>
                setWizard((s) => ({
                  ...s,
                  selectedMcpServers: s.selectedMcpServers.includes(name)
                    ? s.selectedMcpServers.filter((row) => row !== name)
                    : [...s.selectedMcpServers, name],
                }))
              }
              onOpenMcpSettings={() => navigate("mcp")}
              workspaceRootError={workspaceRootError}
            />
          ) : (
            <Step4Review
              wizard={wizard}
              onToggleExportPackDraft={() =>
                setWizard((s) => ({ ...s, exportPackDraft: !s.exportPackDraft }))
              }
              onSubmit={() => deployMutation.mutate()}
              isPending={deployMutation.isPending}
              planPreview={planPreview}
              isPreviewing={compileMutation.isPending}
              planningConversation={planningConversation}
              planningChangeSummary={planningChangeSummary}
              onSendPlanningMessage={(message) => {
                void planningMessageMutation.mutateAsync(message);
              }}
              isSendingPlanningMessage={planningMessageMutation.isPending}
              onResetPlanningChat={() => {
                void planningResetMutation.mutateAsync();
              }}
              isResettingPlanningChat={planningResetMutation.isPending}
              plannerError={plannerError}
            />
          )}
        </motion.div>
      </AnimatePresence>

      {/* Navigation */}
      {step < 4 ? (
        <div className="flex justify-between gap-2">
          <button
            className="tcp-btn"
            disabled={step === 1}
            onClick={() => setStep((s) => (s - 1) as WizardStep)}
          >
            ← Back
          </button>
          <button
            className="tcp-btn-primary"
            disabled={!canAdvance}
            onClick={() => {
              void goToNextStep();
            }}
          >
            Next →
          </button>
        </div>
      ) : null}
    </div>
  );
}

// ─── My Automations (combined routines + packs) ─────────────────────────────

function MyAutomations({
  client,
  toast,
  navigate,
  viewMode,
}: {
  client: any;
  toast: any;
  navigate: (route: string) => void;
  viewMode: "list" | "running";
}) {
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [editDraft, setEditDraft] = useState<{
    automationId: string;
    name: string;
    objective: string;
    mode: "standalone" | "orchestrated";
    requiresApproval: boolean;
    scheduleKind: "cron" | "interval";
    cronExpression: string;
    intervalSeconds: string;
  } | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string>("");
  const [selectedLogSource, setSelectedLogSource] = useState<"all" | "automations" | "global">(
    "all"
  );
  const [runEvents, setRunEvents] = useState<
    Array<{ id: string; source: "automations" | "global"; at: number; event: any }>
  >([]);
  const [selectedSessionId, setSelectedSessionId] = useState<string>("");
  const [sessionEvents, setSessionEvents] = useState<Array<{ id: string; at: number; event: any }>>(
    []
  );
  const sessionLogRef = useRef<HTMLDivElement | null>(null);
  const [sessionLogPinnedToBottom, setSessionLogPinnedToBottom] = useState(true);

  const automationsQuery = useQuery({
    queryKey: ["automations", "list"],
    queryFn: () =>
      client?.automations?.list?.().catch(() => ({ automations: [] })) ??
      Promise.resolve({ automations: [] }),
    refetchInterval: 20000,
  });
  const automationsV2Query = useQuery({
    queryKey: ["automations", "v2", "list"],
    queryFn: () =>
      client?.automationsV2?.list?.().catch(() => ({ automations: [] })) ??
      Promise.resolve({ automations: [] }),
    refetchInterval: 20000,
  });
  const runsQuery = useQuery({
    queryKey: ["automations", "runs"],
    queryFn: () =>
      client?.automations?.listRuns?.({ limit: 20 }).catch(() => ({ runs: [] })) ??
      Promise.resolve({ runs: [] }),
    refetchInterval: 9000,
  });
  const runDetailQuery = useQuery({
    queryKey: ["automations", "run", selectedRunId],
    enabled: !!selectedRunId,
    queryFn: () => client?.automations?.getRun?.(selectedRunId).catch(() => ({ run: null })),
    refetchInterval: selectedRunId ? 5000 : false,
  });
  const runArtifactsQuery = useQuery({
    queryKey: ["automations", "run", "artifacts", selectedRunId],
    enabled: !!selectedRunId,
    queryFn: () =>
      client?.automations?.listArtifacts?.(selectedRunId).catch(() => ({ artifacts: [] })),
    refetchInterval: selectedRunId ? 8000 : false,
  });
  const availableSessionIds = useMemo(
    () => extractSessionIdsFromRun((runDetailQuery.data as any)?.run),
    [runDetailQuery.data]
  );
  const sessionMessagesQuery = useQuery({
    queryKey: ["automations", "run", "session", selectedRunId, selectedSessionId, "messages"],
    enabled: !!selectedRunId && !!selectedSessionId,
    queryFn: () =>
      client?.sessions?.messages?.(selectedSessionId).catch(() => []) ?? Promise.resolve([]),
    refetchInterval:
      selectedRunId &&
      selectedSessionId &&
      isActiveRunStatus((runDetailQuery.data as any)?.run?.status)
        ? 4000
        : false,
  });
  const selectedAutomationId = String(
    (runDetailQuery.data as any)?.run?.automation_id ||
      (runDetailQuery.data as any)?.run?.routine_id ||
      ""
  ).trim();
  const runHistoryQuery = useQuery({
    queryKey: ["automations", "history", selectedAutomationId],
    enabled: !!selectedAutomationId,
    queryFn: () =>
      client?.automations?.history?.(selectedAutomationId, 80).catch(() => ({ events: [] })),
    refetchInterval: selectedRunId ? 10000 : false,
  });
  const packsQuery = useQuery({
    queryKey: ["automations", "packs"],
    queryFn: () =>
      client?.packs?.list?.().catch(() => ({ packs: [] })) ?? Promise.resolve({ packs: [] }),
    refetchInterval: 30000,
  });

  const runNowMutation = useMutation({
    mutationFn: (id: string) => client?.automations?.runNow?.(id),
    onSuccess: async () => {
      toast("ok", "Automation triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const runActionMutation = useMutation({
    mutationFn: async ({
      action,
      runId,
      reason,
    }: {
      action: "pause" | "resume";
      runId: string;
      reason?: string;
    }) => {
      if (action === "pause") return client.automations.pauseRun(runId, reason);
      return client.automations.resumeRun(runId, reason);
    },
    onSuccess: async () => {
      toast("ok", "Run action applied.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const updateAutomationMutation = useMutation({
    mutationFn: async (draft: {
      automationId: string;
      name: string;
      objective: string;
      mode: "standalone" | "orchestrated";
      requiresApproval: boolean;
      scheduleKind: "cron" | "interval";
      cronExpression: string;
      intervalSeconds: string;
    }) => {
      const name = String(draft.name || "").trim();
      const objective = String(draft.objective || "").trim();
      const cronExpression = String(draft.cronExpression || "").trim();
      const intervalSeconds = Number(draft.intervalSeconds);
      if (!name) throw new Error("Automation name is required.");
      if (!objective) throw new Error("Objective is required.");
      if (draft.scheduleKind === "cron" && !cronExpression) {
        throw new Error("Cron expression is required.");
      }
      if (
        draft.scheduleKind === "interval" &&
        (!Number.isFinite(intervalSeconds) || intervalSeconds <= 0)
      ) {
        throw new Error("Interval seconds must be greater than zero.");
      }
      return client.automations.update(draft.automationId, {
        name,
        mode: draft.mode,
        mission: { objective },
        policy: { approval: { requires_approval: !!draft.requiresApproval } },
        schedule:
          draft.scheduleKind === "cron"
            ? { cron: { expression: cronExpression } }
            : { interval_seconds: { seconds: Math.round(intervalSeconds) } },
      });
    },
    onSuccess: async () => {
      toast("ok", "Automation updated.");
      setEditDraft(null);
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const automationActionMutation = useMutation({
    mutationFn: async ({
      action,
      automationId,
      family,
    }: {
      action: "pause" | "resume" | "delete";
      automationId: string;
      family: "legacy" | "v2";
    }) => {
      if (family === "v2") {
        if (action === "delete") return client.automationsV2.delete(automationId);
        if (action === "pause") return client.automationsV2.pause(automationId);
        return client.automationsV2.resume(automationId);
      }
      if (action === "delete") return client.automations.delete(automationId);
      return client.automations.update(automationId, {
        status: action === "pause" ? "paused" : "enabled",
      });
    },
    onSuccess: async (_payload, vars) => {
      if (vars.action === "delete") toast("ok", "Automation removed.");
      if (vars.action === "pause") toast("ok", "Automation paused.");
      if (vars.action === "resume") toast("ok", "Automation resumed.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const automations = useMemo(() => {
    const merged = [
      ...toArray(automationsQuery.data, "automations"),
      ...toArray(automationsQuery.data, "routines"),
    ];
    const byId = new Map<string, any>();
    for (const row of merged) {
      const id = String(row?.automation_id || row?.routine_id || row?.id || "").trim();
      if (!id) continue;
      if (!byId.has(id)) byId.set(id, row);
    }
    return Array.from(byId.values());
  }, [automationsQuery.data]);
  const runs = toArray(runsQuery.data, "runs");
  const automationsV2 = toArray(automationsV2Query.data, "automations");
  const packs = toArray(packsQuery.data, "packs");
  const activeRuns = runs.filter((run: any) => isActiveRunStatus(run?.status));
  const selectedRun = (runDetailQuery.data as any)?.run || null;
  const runArtifacts = toArray(runArtifactsQuery.data, "artifacts");
  const runHints = deriveRunDebugHints(selectedRun, runArtifacts);
  const runHistoryEvents = Array.isArray((runHistoryQuery.data as any)?.events)
    ? (runHistoryQuery.data as any).events
    : Array.isArray((runHistoryQuery.data as any)?.history)
      ? (runHistoryQuery.data as any).history
      : [];
  const filteredRunEvents = runEvents.filter((item) =>
    selectedLogSource === "all" ? true : item.source === selectedLogSource
  );
  const sessionMessages = Array.isArray(sessionMessagesQuery.data) ? sessionMessagesQuery.data : [];

  useEffect(() => {
    setSelectedSessionId((current) => {
      if (current && availableSessionIds.includes(current)) return current;
      return availableSessionIds[0] || "";
    });
  }, [availableSessionIds]);

  useEffect(() => {
    setRunEvents([]);
    setSelectedLogSource("all");
    setSessionEvents([]);
    setSessionLogPinnedToBottom(true);
  }, [selectedRunId]);

  useEngineStream(
    selectedRunId
      ? `/api/engine/automations/events?run_id=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload || payload.status === "ready") return;
        const runId = eventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = eventType(payload);
        const at = eventAt(payload);
        const id = `automations:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        setRunEvents((prev) => [
          ...prev.slice(-299),
          { id, source: "automations", at, event: payload },
        ]);
      } catch {
        return;
      }
    },
    { enabled: !!selectedRunId }
  );

  useEngineStream(
    selectedRunId && selectedSessionId
      ? `/event?sessionID=${encodeURIComponent(selectedSessionId)}&runID=${encodeURIComponent(selectedRunId)}`
      : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        if (!payload) return;
        const type = eventType(payload);
        const at = eventAt(payload);
        const id = [
          type || "event",
          String(payload?.properties?.sessionID || payload?.sessionID || selectedSessionId || ""),
          String(payload?.properties?.runID || payload?.runID || selectedRunId || ""),
          String(payload?.properties?.messageID || payload?.messageID || ""),
          String(
            payload?.properties?.part?.id || payload?.properties?.seq || payload?.timestamp_ms || at
          ),
        ].join(":");
        setSessionEvents((prev) => {
          if (prev.some((row) => row.id === id)) return prev;
          return [...prev.slice(-499), { id, at, event: payload }];
        });
      } catch {
        return;
      }
    },
    { enabled: !!selectedRunId && !!selectedSessionId }
  );

  useEngineStream(
    selectedRunId ? "/api/global/event" : "",
    (msg) => {
      try {
        const payload = JSON.parse(String(msg?.data || "{}"));
        const runId = eventRunId(payload);
        if (!runId || runId !== selectedRunId) return;
        const type = eventType(payload);
        if (!type || type === "server.connected" || type === "engine.lifecycle.ready") return;
        const at = eventAt(payload);
        const id = `global:${runId}:${type}:${at}:${Math.random().toString(16).slice(2, 8)}`;
        setRunEvents((prev) => [...prev.slice(-299), { id, source: "global", at, event: payload }]);
      } catch {
        return;
      }
    },
    { enabled: !!selectedRunId }
  );

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    renderIcons(root);
  }, [
    automations.length,
    automationsV2.length,
    runs.length,
    packs.length,
    activeRuns.length,
    !!editDraft,
    !!selectedRunId,
    !!selectedSessionId,
    runEvents.length,
    sessionEvents.length,
    updateAutomationMutation.isPending,
    runActionMutation.isPending,
    runNowMutation.isPending,
  ]);

  const statusColor = (status: string) => {
    const s = String(status || "").toLowerCase();
    if (s === "active" || s === "completed" || s === "done") return "tcp-badge-ok";
    if (s === "running" || s === "in_progress") return "tcp-badge-warn";
    if (s === "failed" || s === "error") return "tcp-badge-err";
    return "tcp-badge-info";
  };

  const beginEdit = (automation: any) => {
    const automationId = String(
      automation?.automation_id || automation?.id || automation?.routine_id || ""
    ).trim();
    if (!automationId) {
      toast("err", "Cannot edit automation without an id.");
      return;
    }
    const scheduleEditor = scheduleToEditor(automation?.schedule);
    setEditDraft({
      automationId,
      name: String(automation?.name || automationId || "").trim(),
      objective: String(
        automation?.mission?.objective || automation?.mission_snapshot?.objective || ""
      ).trim(),
      mode:
        String(automation?.mode || "").toLowerCase() === "standalone"
          ? "standalone"
          : "orchestrated",
      requiresApproval:
        automation?.requires_approval === true ||
        automation?.policy?.approval?.requires_approval === true,
      scheduleKind: scheduleEditor.scheduleKind,
      cronExpression: scheduleEditor.cronExpression,
      intervalSeconds: String(scheduleEditor.intervalSeconds),
    });
  };

  const isPausedAutomation = (automation: any) => {
    const status = String(automation?.status || "")
      .trim()
      .toLowerCase();
    return status === "paused" || status === "disabled";
  };

  const legacyAutomationCount = automations.length;
  const workflowAutomationCount = automationsV2.length;
  const totalSavedAutomations = legacyAutomationCount + workflowAutomationCount;
  const blockers = useMemo(
    () => buildRunBlockers(selectedRun, sessionEvents, runEvents),
    [selectedRun, sessionEvents, runEvents]
  );
  const sessionLogEntries = useMemo(() => {
    const messageEntries = sessionMessages.map((message: any, index: number) => ({
      id: `message:${String(message?.info?.id || index)}`,
      kind: "message" as const,
      at: normalizeTimestamp(message?.info?.time?.created),
      variant: sessionMessageVariant(message),
      label: String(message?.info?.role || "session").trim() || "session",
      body: sessionMessageText(message),
      raw: message,
      parts: sessionMessageParts(message),
    }));
    const liveEntries = sessionEvents.map((item) => ({
      id: `event:${item.id}`,
      kind: "event" as const,
      at: item.at,
      variant:
        eventType(item.event) === "session.error"
          ? "error"
          : eventType(item.event).startsWith("session.")
            ? "system"
            : "tool",
      label: eventType(item.event) || "event",
      body: eventReason(item.event),
      raw: item.event,
      parts: [],
    }));
    return [...messageEntries, ...liveEntries].sort((a, b) => a.at - b.at);
  }, [sessionMessages, sessionEvents]);

  useEffect(() => {
    const el = sessionLogRef.current;
    if (!el || !sessionLogPinnedToBottom) return;
    el.scrollTop = el.scrollHeight;
  }, [sessionLogEntries, sessionLogPinnedToBottom]);

  return (
    <div ref={rootRef} className="grid gap-4">
      {/* Installed packs from pack_builder */}
      {viewMode === "list" && packs.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Installed Packs
          </p>
          {packs.map((pack: any, i: number) => (
            <div key={String(pack?.id || pack?.name || i)} className="tcp-list-item">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span>📦</span>
                  <strong>{String(pack?.name || pack?.id || "Pack")}</strong>
                </div>
                <span className="tcp-badge-info">{String(pack?.version || "1.0.0")}</span>
              </div>
              <div className="tcp-subtle text-xs mt-1">
                {String(pack?.description || pack?.path || "")}
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {viewMode === "list" ? (
        <div className="grid gap-2">
          <div className="flex items-center justify-between gap-2">
            <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
              Saved Automations
            </p>
            <span className="tcp-badge-info">{totalSavedAutomations} saved</span>
          </div>

          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
                Scheduled Automations
              </p>
              <span className="tcp-subtle text-xs">{legacyAutomationCount} items</span>
            </div>
            {automations.length > 0 ? (
              automations.map((automation: any) => {
                const id = String(
                  automation?.automation_id || automation?.id || automation?.routine_id || ""
                );
                return (
                  <div key={id} className="tcp-list-item">
                    <div className="mb-1 flex items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <span>⏰</span>
                        <strong>{String(automation?.name || id || "Automation")}</strong>
                      </div>
                      <div className="flex items-center gap-2">
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => beginEdit(automation)}
                        >
                          <i data-lucide="pencil"></i>
                        </button>
                        <span className={statusColor(automation?.status)}>
                          {String(automation?.status || "active")}
                        </span>
                      </div>
                    </div>
                    <div className="tcp-subtle text-xs">
                      {formatScheduleLabel(automation?.schedule)}
                    </div>
                    <div className="mt-2">
                      <div className="flex flex-wrap gap-2">
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => runNowMutation.mutate(id)}
                        >
                          <i data-lucide="play"></i>
                          Run now
                        </button>
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() =>
                            automationActionMutation.mutate({
                              action: isPausedAutomation(automation) ? "resume" : "pause",
                              automationId: id,
                              family: "legacy",
                            })
                          }
                          disabled={!id || automationActionMutation.isPending}
                        >
                          <i data-lucide={isPausedAutomation(automation) ? "play" : "pause"}></i>
                          {isPausedAutomation(automation) ? "Resume" : "Pause"}
                        </button>
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => {
                            const latestForAutomation = runs.find((run: any) => {
                              const automationId = String(
                                run?.automation_id || run?.routine_id || run?.id || ""
                              ).trim();
                              return automationId === id;
                            });
                            const runId = String(
                              latestForAutomation?.run_id || latestForAutomation?.id || ""
                            ).trim();
                            if (runId) {
                              setSelectedRunId(runId);
                            } else {
                              toast("info", "No runs yet for this automation.");
                            }
                          }}
                        >
                          <i data-lucide="info"></i>
                          Debug latest
                        </button>
                        <button
                          className="tcp-btn-danger h-7 px-2 text-xs"
                          onClick={() =>
                            automationActionMutation.mutate({
                              action: "delete",
                              automationId: id,
                              family: "legacy",
                            })
                          }
                          disabled={!id || automationActionMutation.isPending}
                        >
                          <i data-lucide="trash-2"></i>
                          Remove
                        </button>
                      </div>
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="tcp-list-item">
                <div className="font-medium">No scheduled automations saved yet</div>
                <div className="tcp-subtle mt-1 text-xs">
                  This section shows automation definitions, not execution history.
                </div>
              </div>
            )}
          </div>
        </div>
      ) : null}

      {viewMode === "list" ? (
        <div className="grid gap-2">
          <div className="flex items-center justify-between gap-2">
            <p className="text-[11px] font-medium uppercase tracking-[0.24em] text-slate-500">
              Workflow Automations
            </p>
            <span className="tcp-subtle text-xs">{workflowAutomationCount} items</span>
          </div>
          {automationsV2.length > 0 ? (
            automationsV2.map((automation: any) => {
              const id = String(automation?.automation_id || automation?.automationId || "").trim();
              const status = String(automation?.status || "draft").trim();
              const paused = status.toLowerCase() === "paused";
              return (
                <div key={id} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2">
                      <span>🧩</span>
                      <strong>{String(automation?.name || id || "Workflow automation")}</strong>
                    </div>
                    <span className={statusColor(status)}>{status}</span>
                  </div>
                  {String(automation?.description || "").trim() ? (
                    <div className="tcp-subtle text-xs">{String(automation.description)}</div>
                  ) : null}
                  <div className="tcp-subtle mt-1 text-xs">
                    {formatAutomationV2ScheduleLabel(automation?.schedule)}
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() =>
                        client.automationsV2
                          .runNow(id)
                          .then(() => {
                            toast("ok", "Workflow automation triggered.");
                            return queryClient.invalidateQueries({ queryKey: ["automations"] });
                          })
                          .catch((error: any) =>
                            toast("err", error instanceof Error ? error.message : String(error))
                          )
                      }
                      disabled={!id}
                    >
                      <i data-lucide="play"></i>
                      Run now
                    </button>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() =>
                        automationActionMutation.mutate({
                          action: paused ? "resume" : "pause",
                          automationId: id,
                          family: "v2",
                        })
                      }
                      disabled={!id || automationActionMutation.isPending}
                    >
                      <i data-lucide={paused ? "play" : "pause"}></i>
                      {paused ? "Resume" : "Pause"}
                    </button>
                    <button
                      className="tcp-btn-danger h-7 px-2 text-xs"
                      onClick={() =>
                        automationActionMutation.mutate({
                          action: "delete",
                          automationId: id,
                          family: "v2",
                        })
                      }
                      disabled={!id || automationActionMutation.isPending}
                    >
                      <i data-lucide="trash-2"></i>
                      Remove
                    </button>
                  </div>
                </div>
              );
            })
          ) : (
            <div className="tcp-list-item">
              <div className="font-medium">No workflow automations saved yet</div>
              <div className="tcp-subtle mt-1 text-xs">
                This section is separate from run history and only shows workflow automation
                definitions.
              </div>
            </div>
          )}
        </div>
      ) : null}

      {viewMode === "running" ? (
        activeRuns.length > 0 ? (
          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-2">
              <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
                Active Running Tasks
              </p>
              <span className="tcp-badge-warn">{activeRuns.length} active</span>
            </div>
            {activeRuns.slice(0, 14).map((run: any, index: number) => {
              const runId = String(run?.run_id || run?.id || index).trim();
              return (
                <div key={runId || index} className="tcp-list-item">
                  <div className="flex items-center justify-between gap-2">
                    <div className="grid gap-0.5">
                      <span className="font-medium text-sm">
                        {String(run?.name || run?.automation_id || run?.routine_id || "Run")}
                      </span>
                      <span className="tcp-subtle text-xs">
                        {runId || "unknown run"} · running for {runTimeLabel(run)}
                      </span>
                    </div>
                    <span className={statusColor(run?.status)}>
                      {String(run?.status || "unknown")}
                    </span>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => setSelectedRunId(runId)}
                    >
                      <i data-lucide="bug"></i>
                      Inspect
                    </button>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => runActionMutation.mutate({ action: "pause", runId })}
                      disabled={!runId || runActionMutation.isPending}
                    >
                      <i data-lucide="pause"></i>
                      Pause
                    </button>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => runActionMutation.mutate({ action: "resume", runId })}
                      disabled={!runId || runActionMutation.isPending}
                    >
                      <i data-lucide="play"></i>
                      Resume
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="tcp-list-item">
            <div className="font-medium">Active Running Tasks</div>
            <div className="tcp-subtle mt-1 text-xs">
              No active runs right now. Start a run to inspect live task execution.
            </div>
          </div>
        )
      ) : null}

      {/* Recent run history */}
      {runs.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            {viewMode === "running" ? "Run Log Explorer" : "Recent Runs"}
          </p>
          {runs.slice(0, 12).map((run: any, index: number) => (
            <div key={String(run?.run_id || run?.id || index)} className="tcp-list-item">
              <div className="flex items-center justify-between gap-2">
                <span className="font-medium text-sm">
                  {String(run?.name || run?.automation_id || run?.routine_id || "Run")}
                </span>
                <span className={statusColor(run?.status)}>{String(run?.status || "unknown")}</span>
              </div>
              <div className="mt-1 flex items-center justify-between gap-2">
                <span className="tcp-subtle text-xs">{String(run?.run_id || run?.id || "")}</span>
                <button
                  className="tcp-btn h-7 px-2 text-xs"
                  onClick={() => setSelectedRunId(String(run?.run_id || run?.id || "").trim())}
                >
                  <i data-lucide="info"></i>
                  {viewMode === "running" ? "View logs" : "Details"}
                </button>
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {!runs.length && viewMode === "running" ? (
        <EmptyState text="Run one automation, then use View logs to inspect full execution events." />
      ) : null}
      {!totalSavedAutomations && !packs.length && !runs.length && viewMode === "list" ? (
        <EmptyState text="No automations yet. Create your first one with the wizard!" />
      ) : null}
      <AnimatePresence>
        {selectedRunId ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => setSelectedRunId("")}
          >
            <motion.div
              className="tcp-confirm-dialog max-h-[88vh] w-[min(110rem,97vw)] overflow-hidden"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
              onClick={(event) => event.stopPropagation()}
            >
              <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                <div className="grid gap-1">
                  <h3 className="tcp-confirm-title">Run Debugger</h3>
                  <div className="tcp-subtle text-xs">
                    automation:{" "}
                    {String(selectedRun?.automation_id || selectedRun?.routine_id || "unknown")}
                    {" · "}run: {selectedRunId}
                    {" · "}running for {runTimeLabel(selectedRun)}
                  </div>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <span className={statusColor(selectedRun?.status)}>
                    {String(selectedRun?.status || "unknown")}
                  </span>
                  {availableSessionIds.length > 1 ? (
                    <select
                      className="tcp-input h-8 min-w-[16rem] text-xs"
                      value={selectedSessionId}
                      onInput={(e) => setSelectedSessionId((e.target as HTMLSelectElement).value)}
                    >
                      {availableSessionIds.map((sessionId) => (
                        <option key={sessionId} value={sessionId}>
                          {sessionId}
                        </option>
                      ))}
                    </select>
                  ) : selectedSessionId ? (
                    <span className="tcp-badge-info">{selectedSessionId}</span>
                  ) : (
                    <span className="tcp-badge-warn">no session</span>
                  )}
                  <button
                    className="tcp-btn h-8 px-2 text-xs"
                    onClick={() => {
                      void Promise.all([
                        queryClient.invalidateQueries({
                          queryKey: ["automations", "run", selectedRunId],
                        }),
                        queryClient.invalidateQueries({
                          queryKey: ["automations", "run", "artifacts", selectedRunId],
                        }),
                        selectedSessionId
                          ? queryClient.invalidateQueries({
                              queryKey: [
                                "automations",
                                "run",
                                "session",
                                selectedRunId,
                                selectedSessionId,
                                "messages",
                              ],
                            })
                          : Promise.resolve(),
                      ]);
                    }}
                  >
                    <i data-lucide="refresh-cw"></i>
                    Refresh
                  </button>
                  <button className="tcp-btn h-8 px-2 text-xs" onClick={() => setSelectedRunId("")}>
                    <i data-lucide="x"></i>
                    Close
                  </button>
                </div>
              </div>
              <div className="grid max-h-[calc(88vh-8rem)] gap-3 overflow-hidden xl:grid-cols-[1.62fr_1fr]">
                <div className="grid min-h-0 gap-3 overflow-hidden">
                  {blockers.length ? (
                    <div className="tcp-list-item">
                      <div className="mb-2 font-medium">Blockers</div>
                      <div className="grid gap-2">
                        {blockers.map((blocker) => (
                          <div
                            key={blocker.key}
                            className="rounded-lg border border-amber-500/30 bg-amber-950/20 p-3"
                          >
                            <div className="mb-1 flex flex-wrap items-center gap-2">
                              <strong>{blocker.title}</strong>
                              <span className="tcp-badge-warn">{blocker.source}</span>
                              {blocker.at ? (
                                <span className="tcp-subtle text-[11px]">
                                  {new Date(blocker.at).toLocaleTimeString()}
                                </span>
                              ) : null}
                            </div>
                            <div className="text-sm text-amber-100/90">{blocker.reason}</div>
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  <div className="tcp-list-item min-h-0 overflow-hidden">
                    <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <div className="font-medium">Live Session Log</div>
                        <div className="tcp-subtle text-xs">
                          {selectedSessionId
                            ? `Streaming session ${selectedSessionId}`
                            : "This run does not expose a session transcript."}
                        </div>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          disabled={!sessionLogEntries.length}
                          onClick={async () => {
                            try {
                              await navigator.clipboard.writeText(
                                sessionLogEntries
                                  .map((entry) => {
                                    const ts = new Date(entry.at).toLocaleTimeString();
                                    return `[${ts}] ${entry.label}\n${entry.body || formatJson(entry.raw)}`;
                                  })
                                  .join("\n\n")
                              );
                              toast("ok", "Copied session log.");
                            } catch (error) {
                              toast("err", error instanceof Error ? error.message : "Copy failed.");
                            }
                          }}
                        >
                          <i data-lucide="copy"></i>
                          Copy session log
                        </button>
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => {
                            setSessionLogPinnedToBottom(true);
                            const el = sessionLogRef.current;
                            if (el) el.scrollTop = el.scrollHeight;
                          }}
                        >
                          <i data-lucide="arrow-down"></i>
                          Jump to latest
                        </button>
                      </div>
                    </div>
                    <div
                      ref={sessionLogRef}
                      className="grid max-h-[30rem] min-h-[18rem] gap-2 overflow-auto pr-1"
                      onScroll={(event) => {
                        const el = event.currentTarget;
                        const pinned = el.scrollHeight - (el.scrollTop + el.clientHeight) < 48;
                        setSessionLogPinnedToBottom(pinned);
                      }}
                    >
                      {sessionLogEntries.length ? (
                        sessionLogEntries.map((entry) => (
                          <div
                            key={entry.id}
                            className={`rounded-lg border p-3 ${
                              entry.variant === "assistant"
                                ? "border-sky-500/30 bg-sky-950/10"
                                : entry.variant === "user"
                                  ? "border-slate-600/60 bg-slate-900/35"
                                  : entry.variant === "error"
                                    ? "border-rose-500/35 bg-rose-950/20"
                                    : "border-slate-700/50 bg-slate-900/25"
                            }`}
                          >
                            <div className="mb-1 flex flex-wrap items-center justify-between gap-2">
                              <span className="text-xs font-medium uppercase tracking-wide text-slate-200">
                                {entry.label}
                              </span>
                              <span className="tcp-subtle text-[11px]">
                                {new Date(entry.at).toLocaleTimeString()}
                              </span>
                            </div>
                            {entry.body ? (
                              <div className="whitespace-pre-wrap break-words text-sm text-slate-100">
                                {entry.body}
                              </div>
                            ) : (
                              <div className="tcp-subtle text-xs">No textual body.</div>
                            )}
                            {entry.kind === "message" &&
                            entry.parts.some((part: any) => String(part?.type || "") === "tool") ? (
                              <details className="mt-2">
                                <summary className="cursor-pointer text-xs text-slate-400">
                                  Tool payloads
                                </summary>
                                <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                  {formatJson(entry.parts)}
                                </pre>
                              </details>
                            ) : null}
                            {entry.kind === "event" ? (
                              <details className="mt-2">
                                <summary className="cursor-pointer text-xs text-slate-400">
                                  Raw event
                                </summary>
                                <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                  {formatJson(entry.raw)}
                                </pre>
                              </details>
                            ) : null}
                          </div>
                        ))
                      ) : (
                        <div className="tcp-subtle text-xs">
                          {selectedSessionId
                            ? "Waiting for session transcript or live session events."
                            : "This run does not expose a session transcript."}
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="tcp-list-item min-h-0 overflow-hidden">
                    <div className="mb-2 flex items-center justify-between gap-2">
                      <div className="font-medium">Run Telemetry</div>
                      <div className="flex flex-wrap gap-1">
                        <button
                          className={`tcp-btn h-7 px-2 text-[11px] ${selectedLogSource === "all" ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                          onClick={() => setSelectedLogSource("all")}
                        >
                          all ({runEvents.length})
                        </button>
                        <button
                          className={`tcp-btn h-7 px-2 text-[11px] ${selectedLogSource === "automations" ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                          onClick={() => setSelectedLogSource("automations")}
                        >
                          automations
                        </button>
                        <button
                          className={`tcp-btn h-7 px-2 text-[11px] ${selectedLogSource === "global" ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""}`}
                          onClick={() => setSelectedLogSource("global")}
                        >
                          global
                        </button>
                      </div>
                    </div>
                    {filteredRunEvents.length ? (
                      <div className="grid max-h-[18rem] gap-2 overflow-auto pr-1">
                        {filteredRunEvents
                          .slice(-40)
                          .reverse()
                          .map((item) => (
                            <details
                              key={item.id}
                              className="rounded-lg border border-slate-700/40 bg-slate-900/30 p-2"
                            >
                              <summary className="cursor-pointer list-none">
                                <div className="flex items-center justify-between gap-2">
                                  <span className="text-xs font-medium text-slate-200">
                                    {eventType(item.event) || "event"}
                                  </span>
                                  <span className="tcp-subtle text-[11px]">
                                    {new Date(item.at).toLocaleTimeString()} · {item.source}
                                  </span>
                                </div>
                                <div className="tcp-subtle mt-1 text-xs">
                                  {eventReason(item.event) || "No summary available."}
                                </div>
                              </summary>
                              <pre className="tcp-code mt-2 max-h-40 overflow-auto text-[11px]">
                                {formatJson(item.event?.properties || item.event)}
                              </pre>
                            </details>
                          ))}
                      </div>
                    ) : (
                      <div className="tcp-subtle text-xs">
                        No automation/global telemetry captured for this run yet.
                      </div>
                    )}
                  </div>
                </div>
                <div className="grid min-h-0 gap-3 overflow-hidden">
                  {runHints.length ? (
                    <div className="tcp-list-item">
                      <div className="mb-1 font-medium">Debug hints</div>
                      <div className="grid gap-1 text-xs text-slate-300">
                        {runHints.map((hint) => (
                          <div key={hint}>{hint}</div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                  <div className="tcp-list-item">
                    <div className="font-medium">Run Summary</div>
                    <div className="mt-2 grid gap-2 text-xs text-slate-300">
                      <div>status: {String(selectedRun?.status || "unknown")}</div>
                      <div>artifacts: {String(runArtifacts.length)}</div>
                      <div>detail: {String(selectedRun?.detail || "n/a")}</div>
                      <div>
                        requires_approval: {String(selectedRun?.requires_approval ?? "unknown")}
                      </div>
                      <div>approval_reason: {String(selectedRun?.approval_reason || "none")}</div>
                      <div>denial_reason: {String(selectedRun?.denial_reason || "none")}</div>
                      <div>paused_reason: {String(selectedRun?.paused_reason || "none")}</div>
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="font-medium">Mission Objective</div>
                    <pre className="tcp-code mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words">
                      {String(
                        selectedRun?.mission_snapshot?.objective || selectedRun?.detail || "n/a"
                      )}
                    </pre>
                  </div>
                  <div className="tcp-list-item">
                    <div className="font-medium">Artifacts ({runArtifacts.length})</div>
                    <pre className="tcp-code mt-2 max-h-40 overflow-auto">
                      {formatJson(runArtifacts)}
                    </pre>
                  </div>
                  <div className="tcp-list-item min-h-0 overflow-hidden">
                    <div className="font-medium">Persisted History</div>
                    {runHistoryEvents.length ? (
                      <div className="mt-2 grid max-h-[14rem] gap-2 overflow-auto pr-1">
                        {runHistoryEvents.map((event: any, index: number) => (
                          <details
                            key={`${String(event?.event || event?.type || "history")}-${index}`}
                            className="rounded-lg border border-slate-700/40 bg-slate-900/25 p-2"
                          >
                            <summary className="cursor-pointer list-none">
                              <div className="flex items-center justify-between gap-2">
                                <span className="text-xs font-medium text-slate-200">
                                  {String(
                                    event?.event || event?.type || event?.status || "history"
                                  )}
                                </span>
                                <span className="tcp-subtle text-[11px]">
                                  {new Date(
                                    normalizeTimestamp(
                                      event?.ts_ms ||
                                        event?.tsMs ||
                                        event?.at ||
                                        event?.timestamp_ms
                                    )
                                  ).toLocaleTimeString()}
                                </span>
                              </div>
                              <div className="tcp-subtle mt-1 text-xs">
                                {String(
                                  event?.detail ||
                                    event?.reason ||
                                    event?.status ||
                                    "No summary available."
                                )}
                              </div>
                            </summary>
                            <pre className="tcp-code mt-2 max-h-32 overflow-auto text-[11px]">
                              {formatJson(event)}
                            </pre>
                          </details>
                        ))}
                      </div>
                    ) : (
                      <div className="tcp-subtle mt-2 text-xs">
                        No persisted history rows returned for this automation.
                      </div>
                    )}
                  </div>
                  <div className="tcp-list-item min-h-0 overflow-hidden">
                    <div className="mb-2 flex items-center justify-between gap-2">
                      <div className="font-medium">Raw Run Payload</div>
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={async () => {
                          try {
                            await navigator.clipboard.writeText(
                              [
                                "=== RUN ===",
                                formatJson(selectedRun),
                                "=== ARTIFACTS ===",
                                formatJson(runArtifacts),
                                "=== TELEMETRY ===",
                                formatJson(filteredRunEvents.map((row) => row.event)),
                                "=== SESSION MESSAGES ===",
                                formatJson(sessionMessages),
                              ].join("\n\n")
                            );
                            toast("ok", "Copied full debug context.");
                          } catch (error) {
                            toast("err", error instanceof Error ? error.message : "Copy failed.");
                          }
                        }}
                      >
                        <i data-lucide="copy-plus"></i>
                        Copy all debug context
                      </button>
                    </div>
                    <pre className="tcp-code max-h-[18rem] overflow-auto">
                      {formatJson(selectedRun)}
                    </pre>
                  </div>
                </div>
              </div>
              <div className="tcp-confirm-actions mt-3">
                <button className="tcp-btn" onClick={() => navigate("feed")}>
                  <i data-lucide="radio"></i>
                  Open Live Feed
                </button>
                <button className="tcp-btn" onClick={() => setSelectedRunId("")}>
                  <i data-lucide="x"></i>
                  Close
                </button>
              </div>
            </motion.div>
          </motion.div>
        ) : null}
        {editDraft ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => setEditDraft(null)}
          >
            <motion.div
              className="tcp-confirm-dialog w-[min(40rem,96vw)]"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
              onClick={(event) => event.stopPropagation()}
            >
              <h3 className="tcp-confirm-title">Edit automation</h3>
              <div className="grid gap-3">
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Name</label>
                  <input
                    className="tcp-input"
                    value={editDraft.name}
                    onInput={(e) =>
                      setEditDraft((current) =>
                        current
                          ? { ...current, name: (e.target as HTMLInputElement).value }
                          : current
                      )
                    }
                  />
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Objective</label>
                  <textarea
                    className="tcp-input min-h-[96px]"
                    value={editDraft.objective}
                    onInput={(e) =>
                      setEditDraft((current) =>
                        current
                          ? { ...current, objective: (e.target as HTMLTextAreaElement).value }
                          : current
                      )
                    }
                  />
                </div>
                <div className="grid gap-1 sm:grid-cols-2 sm:gap-2">
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Mode</label>
                    <select
                      className="tcp-input"
                      value={editDraft.mode}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                mode: (e.target as HTMLSelectElement).value as
                                  | "standalone"
                                  | "orchestrated",
                              }
                            : current
                        )
                      }
                    >
                      <option value="standalone">standalone</option>
                      <option value="orchestrated">orchestrated</option>
                    </select>
                  </div>
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Approval policy</label>
                    <button
                      className={`tcp-input flex h-10 items-center justify-between px-3 text-xs ${
                        editDraft.requiresApproval ? "border-amber-400/60 bg-amber-400/10" : ""
                      }`}
                      role="switch"
                      aria-checked={editDraft.requiresApproval}
                      onClick={() =>
                        setEditDraft((current) =>
                          current
                            ? { ...current, requiresApproval: !current.requiresApproval }
                            : current
                        )
                      }
                    >
                      <span className="flex items-center gap-2">
                        <i
                          data-lucide={editDraft.requiresApproval ? "shield-alert" : "shield-check"}
                        ></i>
                        {editDraft.requiresApproval
                          ? "Manual approvals enabled"
                          : "Fully automated enabled"}
                      </span>
                      <span
                        className={`relative h-5 w-9 rounded-full transition ${
                          editDraft.requiresApproval ? "bg-amber-500/40" : "bg-emerald-500/30"
                        }`}
                      >
                        <span
                          className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-slate-100 transition ${
                            editDraft.requiresApproval ? "" : "translate-x-4"
                          }`}
                        />
                      </span>
                    </button>
                  </div>
                </div>
                <div className="grid gap-1 sm:grid-cols-2 sm:gap-2">
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Schedule type</label>
                    <select
                      className="tcp-input"
                      value={editDraft.scheduleKind}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? {
                                ...current,
                                scheduleKind: (e.target as HTMLSelectElement).value as
                                  | "cron"
                                  | "interval",
                              }
                            : current
                        )
                      }
                    >
                      <option value="interval">interval</option>
                      <option value="cron">cron</option>
                    </select>
                  </div>
                </div>
                {editDraft.scheduleKind === "cron" ? (
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Cron expression</label>
                    <input
                      className="tcp-input font-mono"
                      value={editDraft.cronExpression}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? { ...current, cronExpression: (e.target as HTMLInputElement).value }
                            : current
                        )
                      }
                      placeholder="0 9 * * *"
                    />
                  </div>
                ) : (
                  <div className="grid gap-1">
                    <label className="text-xs text-slate-400">Interval seconds</label>
                    <input
                      type="number"
                      min="1"
                      className="tcp-input"
                      value={editDraft.intervalSeconds}
                      onInput={(e) =>
                        setEditDraft((current) =>
                          current
                            ? { ...current, intervalSeconds: (e.target as HTMLInputElement).value }
                            : current
                        )
                      }
                    />
                  </div>
                )}
              </div>
              <div className="tcp-confirm-actions mt-3">
                <button className="tcp-btn" onClick={() => setEditDraft(null)}>
                  <i data-lucide="x-circle"></i>
                  Cancel
                </button>
                <button
                  className="tcp-btn-primary"
                  onClick={() => editDraft && updateAutomationMutation.mutate(editDraft)}
                  disabled={updateAutomationMutation.isPending}
                >
                  <i data-lucide="check"></i>
                  Save
                </button>
              </div>
            </motion.div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

// ─── Spawn Approvals ────────────────────────────────────────────────────────

function SpawnApprovals({ client, toast }: { client: any; toast: any }) {
  const queryClient = useQueryClient();

  const approvalsQuery = useQuery({
    queryKey: ["automations", "approvals"],
    queryFn: () =>
      client?.agentTeams?.listApprovals?.().catch(() => ({ spawnApprovals: [] })) ??
      Promise.resolve({ spawnApprovals: [] }),
    refetchInterval: 6000,
  });

  const instancesQuery = useQuery({
    queryKey: ["automations", "instances"],
    queryFn: () =>
      client?.agentTeams?.listInstances?.().catch(() => ({ instances: [] })) ??
      Promise.resolve({ instances: [] }),
    refetchInterval: 8000,
  });

  const replyMutation = useMutation({
    mutationFn: ({ requestId, decision }: { requestId: string; decision: "approve" | "deny" }) =>
      client?.agentTeams?.replyApproval?.(requestId, decision),
    onSuccess: async () => {
      toast("ok", "Approval updated.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const approvals = toArray(approvalsQuery.data, "spawnApprovals");
  const instances = toArray(instancesQuery.data, "instances");

  return (
    <div className="grid gap-4">
      {approvals.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Pending Approvals
          </p>
          {approvals.map((approval: any, index: number) => {
            const requestId = String(approval?.request_id || approval?.id || `request-${index}`);
            return (
              <div key={requestId} className="tcp-list-item border-amber-500/40">
                <div className="mb-1 font-medium text-amber-300">
                  ⚠️ {String(approval?.reason || approval?.title || "Spawn request")}
                </div>
                <div className="tcp-subtle text-xs">{requestId}</div>
                <div className="mt-2 flex gap-2">
                  <button
                    className="tcp-btn-primary h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "approve" })}
                  >
                    <i data-lucide="badge-check"></i>
                    Approve
                  </button>
                  <button
                    className="tcp-btn-danger h-7 px-2 text-xs"
                    onClick={() => replyMutation.mutate({ requestId, decision: "deny" })}
                  >
                    <i data-lucide="x"></i>
                    Deny
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      ) : null}

      {instances.length > 0 ? (
        <div className="grid gap-2">
          <p className="text-xs text-slate-500 uppercase tracking-wide font-medium">
            Active Agent Teams
          </p>
          {instances.map((instance: any, index: number) => (
            <div
              key={String(instance?.instance_id || instance?.id || index)}
              className="tcp-list-item"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <span>👥</span>
                  <strong>
                    {String(
                      instance?.name || instance?.template_id || instance?.instance_id || "Instance"
                    )}
                  </strong>
                </div>
                <span className="tcp-badge-info">{String(instance?.status || "active")}</span>
              </div>
              <div className="tcp-subtle text-xs mt-1">
                Mission: {String(instance?.mission_id || "—")}
              </div>
            </div>
          ))}
        </div>
      ) : null}

      {!approvals.length && !instances.length ? (
        <EmptyState text="No pending approvals or active team instances." />
      ) : null}
    </div>
  );
}

// ─── Main Page ──────────────────────────────────────────────────────────────

export function AutomationsPage({ client, toast, navigate, providerStatus }: AppPageProps) {
  const [tab, setTab] = useState<ActiveTab>("create");

  const tabs: { id: ActiveTab; label: string; icon: string }[] = [
    { id: "create", label: "Create New", icon: "sparkles" },
    { id: "list", label: "My Automations", icon: "clipboard-list" },
    { id: "running", label: "Live Tasks", icon: "activity" },
    { id: "approvals", label: "Teams & Approvals", icon: "users" },
  ];

  return (
    <div className="grid gap-4">
      {/* Tab bar */}
      <div className="flex gap-1 rounded-xl border border-slate-700/50 bg-slate-900/40 p-1">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`flex flex-1 items-center justify-center gap-1.5 rounded-lg px-3 py-2 text-sm font-medium transition-all ${
              tab === t.id
                ? "bg-amber-500/20 text-amber-300 shadow-sm"
                : "text-slate-400 hover:text-slate-200"
            }`}
          >
            <i data-lucide={t.icon}></i>
            <span>{t.label}</span>
          </button>
        ))}
      </div>

      {/* Tab content */}
      <AnimatePresence mode="wait">
        <motion.div
          key={tab}
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -6 }}
          transition={{ duration: 0.15 }}
        >
          {tab === "create" ? (
            <PageCard
              title="Create an Automation"
              subtitle="Describe what you want, pick a schedule, and Tandem handles the rest"
            >
              <CreateWizard
                client={client}
                toast={toast}
                navigate={navigate}
                defaultProvider={providerStatus.defaultProvider}
                defaultModel={providerStatus.defaultModel}
              />
            </PageCard>
          ) : tab === "list" ? (
            <PageCard title="My Automations" subtitle="Installed packs, routines and run history">
              <MyAutomations client={client} toast={toast} navigate={navigate} viewMode="list" />
            </PageCard>
          ) : tab === "running" ? (
            <PageCard
              title="Live Running Tasks"
              subtitle="Inspect active runs and open detailed event logs for each run"
            >
              <MyAutomations client={client} toast={toast} navigate={navigate} viewMode="running" />
            </PageCard>
          ) : (
            <PageCard
              title="Teams & Approvals"
              subtitle="Active agent teams and pending spawn approvals"
            >
              <SpawnApprovals client={client} toast={toast} />
            </PageCard>
          )}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
