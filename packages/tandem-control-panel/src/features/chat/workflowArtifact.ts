import type {
  WorkflowPlanStep,
  WorkflowPlannerArtifactLink,
  WorkflowPlannerSessionRecord,
} from "@frumu/tandem-client";

export type WorkflowArtifactNode = {
  id: string;
  kind: string;
  objective: string;
  agentRole: string;
  dependencies: string[];
  output: string;
};

export type WorkflowArtifactStage = {
  id: string;
  parallel: boolean;
  nodes: WorkflowArtifactNode[];
};

export type ChatWorkflowArtifact = {
  sessionId: string;
  title: string;
  description: string;
  revision: number;
  lifecycle: "draft" | "materialized" | "published";
  trigger: string;
  stages: WorkflowArtifactStage[];
  outputs: string[];
  assumptions: string[];
  blockers: string[];
  warnings: string[];
  connections: string[];
  approvals: string[];
  constraints: string[];
  validationStatus: string;
  operationKind: string;
  operationStatus: string;
  operationError: string;
  plannerUrl: string;
  automationUrl: string;
  updatedAtMs: number;
};

type UnknownRecord = Record<string, unknown>;

function asRecord(value: unknown): UnknownRecord {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as UnknownRecord)
    : {};
}

function clean(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function unique(values: string[]): string[] {
  return [...new Set(values.map((value) => value.trim()).filter(Boolean))];
}

function labelsFrom(value: unknown): string[] {
  const rows = Array.isArray(value) ? value : value == null ? [] : [value];
  return unique(
    rows.flatMap((row) => {
      if (typeof row === "string") return [row];
      const record = asRecord(row);
      const label =
        clean(record.message) ||
        clean(record.label) ||
        clean(record.name) ||
        clean(record.capability) ||
        clean(record.code);
      return label ? [label] : [];
    })
  );
}

function nestedValue(record: UnknownRecord, ...path: string[]): unknown {
  let current: unknown = record;
  for (const key of path) current = asRecord(current)[key];
  return current;
}

function issueLabels(value: unknown, blocking: boolean): string[] {
  const rows = Array.isArray(value) ? value : [];
  return labelsFrom(
    rows.filter((row) => {
      const record = asRecord(row);
      return Boolean(record.blocking) === blocking;
    })
  );
}

function stepId(step: WorkflowPlanStep, index: number): string {
  return clean(step.step_id) || clean(step.stepId) || `step-${index + 1}`;
}

function stepDependencies(step: WorkflowPlanStep): string[] {
  const value = step.depends_on ?? step.dependsOn;
  return Array.isArray(value) ? unique(value.map(String)) : [];
}

function stepOutput(step: WorkflowPlanStep): string {
  const contract = step.output_contract ?? step.outputContract;
  return clean(contract?.kind);
}

export function buildWorkflowStages(steps: WorkflowPlanStep[]): WorkflowArtifactStage[] {
  const nodes = steps.map((step, index) => ({
    id: stepId(step, index),
    kind: clean(step.kind) || "task",
    objective: clean(step.objective) || `Workflow step ${index + 1}`,
    agentRole: clean(step.agent_role) || clean(step.agentRole),
    dependencies: stepDependencies(step),
    output: stepOutput(step),
  }));
  const byId = new Map(nodes.map((node) => [node.id, node]));
  const depths = new Map<string, number>();

  const depthOf = (id: string, visiting = new Set<string>()): number => {
    if (depths.has(id)) return depths.get(id) ?? 0;
    if (visiting.has(id)) return 0;
    const node = byId.get(id);
    if (!node) return 0;
    const nextVisiting = new Set(visiting).add(id);
    const parentDepths = node.dependencies
      .filter((dependency) => byId.has(dependency))
      .map((dependency) => depthOf(dependency, nextVisiting));
    const depth = parentDepths.length ? Math.max(...parentDepths) + 1 : 0;
    depths.set(id, depth);
    return depth;
  };

  const grouped = new Map<number, WorkflowArtifactNode[]>();
  for (const node of nodes) {
    const depth = depthOf(node.id);
    grouped.set(depth, [...(grouped.get(depth) ?? []), node]);
  }
  return [...grouped.entries()]
    .sort(([left], [right]) => left - right)
    .map(([depth, stageNodes]) => ({
      id: `stage-${depth}`,
      parallel: stageNodes.length > 1,
      nodes: stageNodes,
    }));
}

function scheduleLabel(value: unknown): string {
  const schedule = asRecord(value);
  const type = clean(schedule.type) || clean(schedule.kind) || "manual";
  const expression =
    clean(schedule.cron) || clean(schedule.expression) || clean(schedule.schedule) || "";
  const event = clean(schedule.event) || clean(schedule.event_type) || clean(schedule.webhook) || "";
  if (type === "manual") return "Manual trigger";
  if (expression) return `${type === "cron" ? "Schedule" : type}: ${expression}`;
  if (event) return `${type}: ${event}`;
  return `${type.charAt(0).toUpperCase()}${type.slice(1)} trigger`;
}

function artifactLink(
  links: WorkflowPlannerArtifactLink[] | undefined,
  kind: string
): WorkflowPlannerArtifactLink | undefined {
  return [...(links ?? [])]
    .filter((link) => clean(link.kind) === kind)
    .sort((left, right) => right.linked_at_ms - left.linked_at_ms)[0];
}

function plannerUrl(value: string, sessionId: string): string {
  const fallback = `/#/planner?session_id=${encodeURIComponent(sessionId)}`;
  if (!value) return fallback;
  if (value.includes("/#/automations?planner_session_id=")) return fallback;
  return value;
}

export function toChatWorkflowArtifact(
  session: WorkflowPlannerSessionRecord
): ChatWorkflowArtifact {
  const draft = session.draft;
  const plan = draft?.current_plan;
  const review = draft?.review;
  const planning = session.planning;
  const metadata = asRecord(plan?.metadata);
  const preview = asRecord(review?.preview_payload);
  const previewValidation = asRecord(preview.validation);
  const validationIssues =
    previewValidation.issues ?? nestedValue(preview, "plan_package_validation", "issues");
  const blockedCapabilities = labelsFrom(review?.blocked_capabilities);
  const blockers = unique([
    ...blockedCapabilities.map((capability) => `Connection required: ${capability}`),
    ...issueLabels(validationIssues, true),
  ]);
  const validationStatus =
    clean(review?.validation_status) ||
    clean(review?.validation_state) ||
    clean(planning?.validation_status) ||
    clean(planning?.validation_state) ||
    (blockers.length ? "blocked" : draft ? "ready" : "planning");
  if (!blockers.length && ["blocked", "failed", "invalid"].includes(validationStatus)) {
    blockers.push("Workflow validation must be resolved before materialization.");
  }

  const requiredCapabilities = labelsFrom(review?.required_capabilities);
  const requestedCapabilities = labelsFrom(review?.requested_capabilities);
  const mcpServers = unique((plan?.allowed_mcp_servers ?? plan?.allowedMcpServers ?? []).map(String));
  const automation = artifactLink(session.artifact_links, "automation");
  const planner = artifactLink(session.artifact_links, "planner_session");
  const lifecycle = session.published_at_ms
    ? "published"
    : automation
      ? "materialized"
      : "draft";
  const steps = plan?.steps ?? [];
  const outputs = unique(
    steps
      .map((step, index) => {
        const output = stepOutput(step);
        return output ? `${stepId(step, index)}: ${output}` : "";
      })
      .filter(Boolean)
  );
  const approvalStatus = clean(review?.approval_status) || clean(planning?.approval_status);
  const executionTarget = clean(plan?.execution_target) || clean(plan?.executionTarget);
  const workspaceRoot = clean(plan?.workspace_root) || clean(plan?.workspaceRoot);
  const operation = session.operation;

  return {
    sessionId: session.session_id,
    title: clean(plan?.title) || clean(session.title) || "Workflow draft",
    description: clean(plan?.description) || clean(session.goal),
    revision: draft?.plan_revision ?? 1,
    lifecycle,
    trigger: scheduleLabel(plan?.schedule),
    stages: buildWorkflowStages(steps),
    outputs,
    assumptions: unique([
      ...labelsFrom(metadata.assumptions),
      ...labelsFrom(preview.assumptions),
      ...labelsFrom(planning?.known_requirements),
    ]),
    blockers,
    warnings: unique([
      ...labelsFrom(metadata.warnings),
      ...labelsFrom(preview.warnings),
      ...issueLabels(validationIssues, false),
      ...labelsFrom(planning?.missing_requirements).map(
        (requirement) => `Missing detail: ${requirement}`
      ),
    ]),
    connections: unique([
      ...requiredCapabilities,
      ...requestedCapabilities,
      ...mcpServers.map((server) => `MCP: ${server}`),
    ]),
    approvals: approvalStatus ? [`Approval: ${approvalStatus}`] : [],
    constraints: unique([
      executionTarget ? `Target: ${executionTarget}` : "",
      workspaceRoot ? `Workspace: ${workspaceRoot}` : "",
      ...labelsFrom(metadata.constraints),
    ]),
    validationStatus,
    operationKind: clean(operation?.kind),
    operationStatus: clean(operation?.status),
    operationError: clean(operation?.error),
    plannerUrl: plannerUrl(clean(planner?.resource_url), session.session_id),
    automationUrl: clean(automation?.resource_url),
    updatedAtMs: session.updated_at_ms,
  };
}
