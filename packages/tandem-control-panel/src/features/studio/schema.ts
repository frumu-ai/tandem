export type StudioRole =
  | "worker"
  | "reviewer"
  | "tester"
  | "watcher"
  | "delegator"
  | "committer"
  | "orchestrator";

export type StudioPromptSections = {
  role: string;
  mission: string;
  inputs: string;
  outputContract: string;
  guardrails: string;
};

export type StudioAgentDraft = {
  agentId: string;
  displayName: string;
  role: StudioRole;
  avatarUrl: string;
  templateId: string;
  linkedTemplateId: string;
  skills: string[];
  prompt: StudioPromptSections;
  modelProvider: string;
  modelId: string;
  toolAllowlist: string[];
  toolDenylist: string[];
  mcpAllowedServers: string[];
};

export type StudioNodeDraft = {
  nodeId: string;
  title: string;
  agentId: string;
  objective: string;
  dependsOn: string[];
  inputRefs: Array<{ fromStepId: string; alias: string }>;
  outputKind: string;
  outputPath: string;
  taskKind?: string;
  projectBacklogTasks?: boolean;
  backlogTaskId?: string;
  repoRoot?: string;
  writeScope?: string;
  acceptanceCriteria?: string;
  taskDependencies?: string;
  verificationState?: string;
  taskOwner?: string;
  verificationCommand?: string;
};

export type StudioWorkflowDraft = {
  automationId: string;
  starterTemplateId: string;
  name: string;
  description: string;
  summary: string;
  icon: string;
  workspaceRoot: string;
  status: "draft" | "active" | "paused";
  scheduleType: "manual" | "cron" | "interval";
  cronExpression: string;
  intervalSeconds: string;
  maxParallelAgents: string;
  useSharedModel: boolean;
  sharedModelProvider: string;
  sharedModelId: string;
  outputTargets: string[];
  agents: StudioAgentDraft[];
  nodes: StudioNodeDraft[];
};

export type StudioTemplateDefinition = {
  id: string;
  name: string;
  icon: string;
  summary: string;
  description: string;
  agents: StudioAgentDraft[];
  nodes: StudioNodeDraft[];
  suggestedOutputs: string[];
};

export function emptyPromptSections(
  overrides: Partial<StudioPromptSections> = {}
): StudioPromptSections {
  return {
    role: "",
    mission: "",
    inputs: "",
    outputContract: "",
    guardrails: "",
    ...overrides,
  };
}
