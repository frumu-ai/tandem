import YAML from "yaml";

export type MissionBuilderScheduleDefaults = {
  type?: "manual" | "interval" | "cron";
  interval_seconds?: number;
  cron_expression?: string;
  timezone?: string;
};

function safeString(value: unknown) {
  return String(value || "").trim();
}

function stripCodeFence(raw: string) {
  const text = String(raw || "").trim();
  const fenced = text.match(/^```(?:ya?ml|json)?\s*([\s\S]*?)\s*```$/i);
  return fenced ? fenced[1].trim() : text;
}

function scheduleHint(
  scheduleKind: "manual" | "interval" | "cron",
  intervalSeconds: string,
  cronExpression: string
) {
  if (scheduleKind === "cron") {
    const expression = safeString(cronExpression);
    return expression ? `cron:${expression}` : "cron";
  }
  if (scheduleKind === "interval") {
    const seconds = Number.parseInt(String(intervalSeconds || "0"), 10) || 0;
    return seconds > 0 ? `interval:${seconds}s` : "interval";
  }
  return "manual";
}

export function buildIntentToMissionBlueprintPrompt(input: {
  missionTitle: string;
  missionGoal: string;
  sharedContext: string;
  successCriteria: string[];
  workspaceRoot: string;
  archetypeLabel?: string;
  scheduleKind: "manual" | "interval" | "cron";
  intervalSeconds: string;
  cronExpression: string;
}) {
  const missionTitle = safeString(input.missionTitle);
  const missionGoal = safeString(input.missionGoal);
  const sharedContext = safeString(input.sharedContext);
  const workspaceRoot = safeString(input.workspaceRoot);
  const archetypeLabel = safeString(input.archetypeLabel);
  const successCriteria = Array.isArray(input.successCriteria)
    ? input.successCriteria.map(safeString).filter(Boolean)
    : [];
  const cadence = scheduleHint(input.scheduleKind, input.intervalSeconds, input.cronExpression);

  return [
    "Design one Tandem MissionBlueprint for the following human intent.",
    "",
    "Return valid YAML only.",
    "Do not return explanation, commentary, markdown fences, or notes outside the YAML.",
    "",
    "Mission compiler requirements:",
    "- Produce one shared mission goal, not a checklist.",
    "- Use 3 to 7 scoped workstreams with one responsibility each.",
    "- Use explicit `depends_on` only for real handoffs.",
    "- Use explicit `input_refs` when a stage needs named upstream outputs.",
    "- Every workstream must include a strong `prompt` and a concrete `output_contract`.",
    "- Add review, test, or approval stages only where they materially improve quality, verification, or promotion control.",
    "- Keep outputs reusable by later stages and future runs.",
    "- Assume this mission may run repeatedly over weeks or months.",
    "- Default to project-scoped knowledge reuse through validated stage outputs, not raw intermediate notes.",
    "- Avoid vague stages like `do work`, `analyze everything`, or `final thoughts`.",
    "- Prefer explicit phases, milestones, and barriers when they improve operational sequencing.",
    "",
    "Prompt-writing requirements for each workstream:",
    "- State the local role clearly.",
    "- State the local objective clearly.",
    "- Specify what inputs are allowed.",
    "- Specify what output must be produced.",
    "- Include guardrails against repeating earlier work, inventing facts, or widening scope unnecessarily.",
    "",
    "Scheduling and recurrence guidance:",
    `- Intended cadence: ${cadence}.`,
    "- If the cadence suggests a recurring mission, design outputs and review points so later runs can build on prior validated state.",
    "- Do not force a review or approval stage after every workstream; place them only where promotion, external action, or downstream trust requires it.",
    "",
    "Human intent:",
    missionTitle
      ? `- Mission title hint: ${missionTitle}`
      : "- Mission title hint: derive a concise operator-friendly title.",
    missionGoal
      ? `- Mission goal: ${missionGoal}`
      : "- Mission goal: derive from the implied objective.",
    sharedContext
      ? `- Shared context: ${sharedContext}`
      : "- Shared context: infer only stable cross-cutting constraints.",
    workspaceRoot
      ? `- Workspace root: ${workspaceRoot}`
      : "- Workspace root: use the provided workspace root.",
    archetypeLabel
      ? `- Archetype hint: ${archetypeLabel}`
      : "- Archetype hint: choose the smallest suitable staged pattern.",
    successCriteria.length
      ? "- Success criteria:\n" + successCriteria.map((item) => `  - ${item}`).join("\n")
      : "- Success criteria: infer measurable outcomes from the goal.",
    "",
    "Return YAML with this shape:",
    "- id",
    "- label",
    "- description",
    "- schedule_defaults",
    "- blueprint",
    "",
    "The `blueprint` must contain:",
    "- mission_id",
    "- title",
    "- goal",
    "- success_criteria",
    "- shared_context",
    "- workspace_root",
    "- phases",
    "- milestones",
    "- team",
    "- workstreams",
    "- review_stages",
    "",
    "Choose realistic roles, dependencies, outputs, and review gates. Optimize for durable staged execution rather than a one-off chat response.",
  ].join("\n");
}

export function parseMissionBlueprintDraft(raw: string): {
  blueprint: Record<string, unknown> | null;
  scheduleDefaults?: MissionBuilderScheduleDefaults;
  error?: string;
} {
  const cleaned = stripCodeFence(raw);
  if (!cleaned) {
    return {
      blueprint: null,
      error: "Paste YAML or JSON containing a mission blueprint draft.",
    };
  }
  let parsed: unknown;
  try {
    parsed = YAML.parse(cleaned);
  } catch (error) {
    return {
      blueprint: null,
      error: error instanceof Error ? error.message : "Unable to parse YAML/JSON draft.",
    };
  }
  if (!parsed || typeof parsed !== "object") {
    return { blueprint: null, error: "The imported draft must be a YAML or JSON object." };
  }
  const candidate = parsed as Record<string, unknown>;
  const blueprint =
    candidate.blueprint && typeof candidate.blueprint === "object"
      ? (candidate.blueprint as Record<string, unknown>)
      : candidate;
  if (!blueprint || typeof blueprint !== "object") {
    return { blueprint: null, error: "No mission blueprint object was found in the draft." };
  }
  const scheduleDefaults =
    candidate.schedule_defaults && typeof candidate.schedule_defaults === "object"
      ? (candidate.schedule_defaults as MissionBuilderScheduleDefaults)
      : undefined;
  return { blueprint, scheduleDefaults };
}

export function applyScheduleDefaultsToEditor(
  defaults: MissionBuilderScheduleDefaults | undefined
): {
  scheduleKind: "manual" | "interval" | "cron";
  intervalSeconds: string;
  cronExpression: string;
} {
  const kind = safeString(defaults?.type).toLowerCase();
  if (kind === "cron") {
    return {
      scheduleKind: "cron",
      intervalSeconds: "3600",
      cronExpression: safeString(defaults?.cron_expression),
    };
  }
  if (kind === "interval") {
    const seconds = Number(defaults?.interval_seconds || 3600);
    return {
      scheduleKind: "interval",
      intervalSeconds: String(Math.max(1, Number.isFinite(seconds) ? seconds : 3600)),
      cronExpression: "",
    };
  }
  return { scheduleKind: "manual", intervalSeconds: "3600", cronExpression: "" };
}

export function missionBuilderKnowledgeGuardrails() {
  return [
    "Generated missions should start from project-scoped promoted knowledge.",
    "Validated outputs should feed later stages and future runs; raw intermediate notes should not.",
    "Recurring missions should prefer stable stage outputs and explicit handoffs over re-discovery.",
  ];
}
