import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useMemo, useState } from "react";
import YAML from "yaml";
import { ChatInterfacePanel, type ChatQuickReply } from "../../../components/ChatInterfacePanel";
import {
  buildDefaultKnowledgeOperatorPreferences,
  buildKnowledgeRolloutGuidance,
  normalizePlannerConversationMessages,
} from "../../planner/plannerShared";
import type { NavigationLockState } from "../../../pages/pageTypes";
import { api } from "../../../lib/api";
import { ExecutionProfileToggle } from "../ExecutionProfileToggle";

type ComposerPhase = "intent_capture" | "clarification" | "draft_ready" | "created";

type ClarifierOption = {
  id: string;
  label: string;
};

type ClarificationState =
  | { status: "none" }
  | {
      status: "waiting";
      question: string;
      options: ClarifierOption[];
    };

type AutomationComposerPanelProps = {
  client: any;
  api: any;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
  defaultProvider: string;
  defaultModel: string;
  onShowAutomations: () => void;
  onShowRuns: () => void;
  onNavigationLockChange?: (lock: NavigationLockState | null) => void;
};

type ComposerStorage = {
  input: string;
  phase: ComposerPhase;
  draftName: string;
  workspaceRoot: string;
  planPreview: any | null;
  conversation: any | null;
  clarification: ClarificationState;
  createdAutomation: any | null;
  createdAutomationId: string;
  payloadMode: "json" | "yaml";
  agentTestMode: boolean;
  agentTestAgentId: string;
};

const AUTOMATION_COMPOSER_STORAGE_KEY = "tandem.automations.composer.v1";
const DEFAULT_AGENT_ID = "control-panel-agent";

function safeString(value: unknown) {
  return String(value || "").trim();
}

function slugify(value: string) {
  const normalized = safeString(value)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return normalized || "step";
}

function titleCase(value: string) {
  const text = safeString(value);
  if (!text) return "Worker";
  return text
    .split(/[_\-\s]+/g)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

function normalizeStringArray(raw: unknown) {
  const rows = Array.isArray(raw) ? raw : [];
  const seen = new Set<string>();
  const values: string[] = [];
  for (const row of rows) {
    const value = safeString(row);
    if (!value || seen.has(value)) continue;
    seen.add(value);
    values.push(value);
  }
  return values;
}

function extractMcpServers(raw: any) {
  if (Array.isArray(raw?.servers)) {
    return raw.servers
      .map((row: any) => safeString(row?.name))
      .filter(Boolean)
      .sort((a: string, b: string) => a.localeCompare(b));
  }
  return Object.keys(raw || {})
    .map((key) => safeString(key))
    .filter(Boolean)
    .sort((a: string, b: string) => a.localeCompare(b));
}

function normalizeSchedule(schedule: any) {
  const type = safeString(schedule?.type || "").toLowerCase();
  const timezone = safeString(schedule?.timezone || "UTC") || "UTC";
  if (type === "cron") {
    return {
      type: "cron",
      cron_expression: safeString(schedule?.cron_expression || schedule?.cronExpression || ""),
      timezone,
      misfire_policy: { type: "run_once" as const },
    };
  }
  if (type === "interval") {
    const intervalSeconds = Math.max(
      1,
      Number.parseInt(
        String(schedule?.interval_seconds || schedule?.intervalSeconds || "3600"),
        10
      ) || 3600
    );
    return {
      type: "interval",
      interval_seconds: intervalSeconds,
      timezone,
      misfire_policy: { type: "run_once" as const },
    };
  }
  return {
    type: "manual",
    timezone,
    misfire_policy: { type: "run_once" as const },
  };
}

function objectiveLooksWritable(objective: string) {
  return /write|create|update|edit|patch|publish|report|summar|artifact|save|export/i.test(
    safeString(objective)
  );
}

function objectiveLooksLikeMcp(objective: string) {
  return /notify|send|slack|mcp|webhook|post/i.test(safeString(objective));
}

function buildAgentProfile(
  role: string,
  objective: string,
  allowedServers: string[],
  defaults: {
    provider: string;
    model: string;
  }
) {
  const cleanRole = safeString(role) || "worker";
  const normalizedServers = normalizeStringArray(allowedServers);
  return {
    agent_id: slugify(cleanRole),
    display_name: titleCase(cleanRole),
    skills: [],
    model_policy:
      defaults.provider && defaults.model
        ? {
            default_model: {
              provider_id: defaults.provider,
              model_id: defaults.model,
            },
          }
        : undefined,
    tool_policy: { allowlist: objectiveLooksWritable(objective) ? ["read", "write"] : ["read"] },
    mcp_policy: {
      allowed_servers: objectiveLooksLikeMcp(objective) ? normalizedServers : [],
      allowed_tools: objectiveLooksLikeMcp(objective) ? ["send_message"] : [],
    },
    approval_policy: "auto",
  };
}

function buildAutomationPayloadFromPlan(input: {
  plan: any;
  prompt: string;
  draftName: string;
  workspaceRoot: string;
  defaultProvider: string;
  defaultModel: string;
  allowedMcpServers: string[];
  creatorId: string;
}) {
  const plan = input.plan || {};
  const steps = Array.isArray(plan?.steps) ? plan.steps : [];
  const planTitle = safeString(
    input.draftName || plan?.title || input.prompt || "AI-first automation"
  );
  const planId = safeString(plan?.plan_id || plan?.planId);
  const workspaceRoot = safeString(
    plan?.workspace_root || plan?.workspaceRoot || input.workspaceRoot
  );
  const mcpServers = normalizeStringArray(
    plan?.allowed_mcp_servers || plan?.allowedMcpServers || input.allowedMcpServers
  );
  const allowedServers = mcpServers.length
    ? mcpServers
    : normalizeStringArray(input.allowedMcpServers);
  const defaults = {
    provider: safeString(input.defaultProvider),
    model: safeString(input.defaultModel),
  };
  const nodes: any[] = [];
  const usedNodeIds = new Set<string>();
  const agentsByRole = new Map<string, any>();

  steps.forEach((step: any, index: number) => {
    const role = safeString(step?.agent_role || step?.agentRole || "worker") || "worker";
    const baseNodeId = slugify(
      step?.step_id || step?.stepId || step?.objective || `step-${index + 1}`
    );
    let nodeId = baseNodeId;
    let suffix = 2;
    while (usedNodeIds.has(nodeId)) {
      nodeId = `${baseNodeId}-${suffix}`;
      suffix += 1;
    }
    usedNodeIds.add(nodeId);
    const objective = safeString(step?.objective || `${role} step ${index + 1}`);
    const dependsOn = normalizeStringArray(step?.depends_on || step?.dependsOn);
    const previousNodeId = nodes[nodes.length - 1]?.nodeId;
    const nodeDependsOn = dependsOn.length ? dependsOn : previousNodeId ? [previousNodeId] : [];

    if (!agentsByRole.has(role)) {
      agentsByRole.set(role, buildAgentProfile(role, objective, allowedServers, defaults));
    } else if (objectiveLooksLikeMcp(objective)) {
      const current = agentsByRole.get(role);
      current.mcp_policy = {
        allowed_servers: allowedServers,
        allowed_tools: ["send_message"],
      };
      agentsByRole.set(role, current);
    }

    nodes.push({
      node_id: nodeId,
      agent_id: slugify(role),
      objective,
      depends_on: nodeDependsOn,
      input_refs: Array.isArray(step?.input_refs || step?.inputRefs)
        ? (step?.input_refs || step?.inputRefs)
            .map((row: any) => ({
              from_step_id: safeString(row?.from_step_id || row?.fromStepId),
              alias: safeString(row?.alias),
            }))
            .filter((row: any) => row.from_step_id && row.alias)
        : undefined,
      output_contract: step?.output_contract || step?.outputContract || { kind: "artifact" },
      retry_policy: step?.retry_policy || step?.retryPolicy || undefined,
      timeout_ms: step?.timeout_ms || step?.timeoutMs || undefined,
    });
  });

  const agents = Array.from(agentsByRole.values());
  if (!agents.length && planTitle) {
    agents.push(buildAgentProfile("worker", planTitle, allowedServers, defaults));
    nodes.push({
      node_id: slugify(planTitle),
      agent_id: slugify("worker"),
      objective: planTitle,
      depends_on: [],
      output_contract: { kind: "artifact" },
    });
  }

  const operatorPreferences = {
    ...buildDefaultKnowledgeOperatorPreferences(planTitle || input.prompt),
    ...buildKnowledgeRolloutGuidance(planTitle || input.prompt),
  };

  return {
    name: planTitle,
    description: safeString(plan?.description || input.prompt) || undefined,
    status: "active",
    schedule: normalizeSchedule(plan?.schedule),
    workspace_root: workspaceRoot,
    creator_id: input.creatorId || "control-panel-composer",
    agents,
    flow: { nodes },
    metadata: {
      composer: {
        source_plan_id: planId || undefined,
        draft_name: input.draftName || undefined,
        prompt: input.prompt,
        created_from: "control-panel-composer",
        allowed_mcp_servers: allowedServers,
      },
      operator_preferences: operatorPreferences,
      docs: {
        handbook: "/docs/automation-composer-workflows/",
      },
    },
  };
}

function automationIdFromResponse(response: any) {
  return safeString(
    response?.automation?.automation_id || response?.automation?.automationId || response?.id
  );
}

function summarizeClarifier(response: any): ClarificationState {
  const question = safeString(response?.clarifier?.question);
  const options = Array.isArray(response?.clarifier?.options)
    ? response.clarifier.options
        .map((row: any) => ({
          id: safeString(row?.id),
          label: safeString(row?.label),
        }))
        .filter((row: ClarifierOption) => row.id && row.label)
    : [];
  if (question && options.length > 0) {
    return { status: "waiting", question, options };
  }
  return { status: "none" };
}

function safeJson(value: any) {
  return JSON.stringify(value, null, 2);
}

function safeYaml(value: any) {
  try {
    return YAML.stringify(value);
  } catch {
    return "";
  }
}

function copyText(text: string) {
  if (typeof navigator === "undefined" || !navigator.clipboard?.writeText)
    return Promise.resolve(false);
  return navigator.clipboard
    .writeText(text)
    .then(() => true)
    .catch(() => false);
}

export function AutomationComposerPanel({
  client,
  api: _api,
  toast,
  defaultProvider,
  defaultModel,
  onShowAutomations,
  onShowRuns,
  onNavigationLockChange,
}: AutomationComposerPanelProps) {
  const queryClient = useQueryClient();
  const [input, setInput] = useState("");
  const [draftName, setDraftName] = useState("");
  const [workspaceRootInput, setWorkspaceRootInput] = useState("");
  const [phase, setPhase] = useState<ComposerPhase>("intent_capture");
  const [planPreview, setPlanPreview] = useState<any | null>(null);
  const [conversation, setConversation] = useState<any | null>(null);
  const [clarification, setClarification] = useState<ClarificationState>({ status: "none" });
  const [createdAutomation, setCreatedAutomation] = useState<any | null>(null);
  const [createdAutomationId, setCreatedAutomationId] = useState("");
  const [payloadMode, setPayloadMode] = useState<"json" | "yaml">("json");
  const [executionProfile, setExecutionProfile] = useState<"" | "strict" | "guided" | "yolo">("");
  const [agentTestMode, setAgentTestMode] = useState(false);
  const [agentTestAgentId, setAgentTestAgentId] = useState(DEFAULT_AGENT_ID);
  const [validationMessage, setValidationMessage] = useState("");

  const healthQuery = useQuery({
    queryKey: ["automations", "composer", "health"],
    queryFn: () => client.workspace().catch(() => ({})),
    refetchInterval: 30000,
  });
  const mcpQuery = useQuery({
    queryKey: ["automations", "composer", "mcp"],
    queryFn: () => client.mcp.list().catch(() => ({})),
    refetchInterval: 12000,
  });
  const runsQuery = useQuery({
    queryKey: ["automations", "composer", "runs", createdAutomationId],
    enabled: !!createdAutomationId && !!client?.automationsV2?.listRuns,
    queryFn: async () => client.automationsV2.listRuns(createdAutomationId, 10),
    refetchInterval: 5000,
  });
  const allowedMcpServers = useMemo(() => extractMcpServers(mcpQuery.data), [mcpQuery.data]);
  const workspaceRootFromHealth = useMemo(
    () =>
      safeString(
        (healthQuery.data as any)?.workspaceRoot || (healthQuery.data as any)?.workspace_root || ""
      ),
    [healthQuery.data]
  );
  const workspaceRoot = useMemo(
    () => safeString(workspaceRootInput || workspaceRootFromHealth),
    [workspaceRootFromHealth, workspaceRootInput]
  );

  const normalizedMessages = useMemo(
    () => normalizePlannerConversationMessages(conversation, true),
    [conversation]
  );
  const generatedPayload = useMemo(() => {
    if (!planPreview) return null;
    return buildAutomationPayloadFromPlan({
      plan: planPreview,
      prompt: input,
      draftName,
      workspaceRoot,
      defaultProvider,
      defaultModel,
      allowedMcpServers,
      creatorId: agentTestMode
        ? safeString(agentTestAgentId) || DEFAULT_AGENT_ID
        : "control-panel-composer",
    });
  }, [
    allowedMcpServers,
    defaultModel,
    defaultProvider,
    draftName,
    input,
    planPreview,
    workspaceRoot,
    agentTestAgentId,
    agentTestMode,
  ]);

  const jsonPreview = useMemo(
    () => (generatedPayload ? safeJson(generatedPayload) : ""),
    [generatedPayload]
  );
  const yamlPreview = useMemo(
    () => (generatedPayload ? safeYaml(generatedPayload) : ""),
    [generatedPayload]
  );
  const previewText = payloadMode === "yaml" ? yamlPreview : jsonPreview;
  const previewLabel = payloadMode === "yaml" ? "YAML" : "JSON";
  const validationErrors = useMemo(() => {
    const errors: string[] = [];
    if (!safeString(input) && !safeString(planPreview?.title)) {
      errors.push("Describe the automation goal first.");
    }
    if (!safeString(workspaceRoot)) {
      errors.push("Workspace root is required before creating an automation.");
    }
    if (workspaceRoot && !workspaceRoot.startsWith("/")) {
      errors.push("Workspace root must be an absolute path.");
    }
    if (!generatedPayload?.agents?.length) {
      errors.push("The generated payload did not produce any agents.");
    }
    if (!generatedPayload?.flow?.nodes?.length) {
      errors.push("The generated payload did not produce any workflow nodes.");
    }
    if (agentTestMode && !safeString(agentTestAgentId)) {
      errors.push("Agent test mode requires an agent id.");
    }
    if (!agentTestMode && !client?.automationsV2?.create) {
      errors.push("This control panel build does not expose automationsV2.create.");
    }
    return errors;
  }, [
    agentTestAgentId,
    agentTestMode,
    client?.automationsV2?.create,
    generatedPayload,
    input,
    planPreview?.title,
    workspaceRoot,
  ]);

  const latestRun = useMemo(() => {
    const runs = Array.isArray(runsQuery.data?.runs) ? runsQuery.data.runs : [];
    return runs[0] || null;
  }, [runsQuery.data]);

  useEffect(() => {
    try {
      const raw = localStorage.getItem(AUTOMATION_COMPOSER_STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as Partial<ComposerStorage>;
      if (typeof parsed.input === "string") setInput(parsed.input);
      if (typeof parsed.draftName === "string") setDraftName(parsed.draftName);
      if (typeof parsed.workspaceRoot === "string") setWorkspaceRootInput(parsed.workspaceRoot);
      if (parsed.planPreview) setPlanPreview(parsed.planPreview);
      if (parsed.conversation) setConversation(parsed.conversation);
      if (parsed.clarification) setClarification(parsed.clarification);
      if (parsed.createdAutomation) setCreatedAutomation(parsed.createdAutomation);
      if (typeof parsed.createdAutomationId === "string")
        setCreatedAutomationId(parsed.createdAutomationId);
      if (parsed.phase) setPhase(parsed.phase);
      if (parsed.payloadMode) setPayloadMode(parsed.payloadMode);
      if (typeof parsed.agentTestMode === "boolean") setAgentTestMode(parsed.agentTestMode);
      if (typeof parsed.agentTestAgentId === "string") setAgentTestAgentId(parsed.agentTestAgentId);
    } catch {
      // Ignore restore failures.
    }
  }, []);

  useEffect(() => {
    if (!workspaceRootInput && workspaceRootFromHealth) {
      setWorkspaceRootInput(workspaceRootFromHealth);
    }
  }, [workspaceRootFromHealth, workspaceRootInput]);

  useEffect(() => {
    try {
      localStorage.setItem(
        AUTOMATION_COMPOSER_STORAGE_KEY,
        JSON.stringify({
          input,
          phase,
          draftName,
          workspaceRoot: workspaceRootInput,
          planPreview,
          conversation,
          clarification,
          createdAutomation,
          createdAutomationId,
          payloadMode,
          agentTestMode,
          agentTestAgentId,
        } as ComposerStorage)
      );
    } catch {
      // Ignore storage failures.
    }
  }, [
    clarification,
    conversation,
    createdAutomation,
    createdAutomationId,
    draftName,
    input,
    payloadMode,
    agentTestAgentId,
    agentTestMode,
    phase,
    planPreview,
    workspaceRootInput,
  ]);

  const resetLocalState = () => {
    setInput("");
    setDraftName("");
    setPhase("intent_capture");
    setPlanPreview(null);
    setConversation(null);
    setClarification({ status: "none" });
    setCreatedAutomation(null);
    setCreatedAutomationId("");
    setAgentTestMode(false);
    setAgentTestAgentId(DEFAULT_AGENT_ID);
    setValidationMessage("");
    try {
      localStorage.removeItem(AUTOMATION_COMPOSER_STORAGE_KEY);
    } catch {
      // ignore
    }
  };

  const startMutation = useMutation({
    mutationFn: async (goal: string) => {
      if (!client?.workflowPlans?.chatStart) {
        throw new Error("Planner chat support is unavailable in this build.");
      }
      const trimmed = safeString(goal);
      if (!trimmed) throw new Error("Describe the automation you want to build.");
      if (!workspaceRoot) {
        throw new Error("Workspace root is required before starting a composer draft.");
      }
      return client.workflowPlans.chatStart({
        prompt: [
          "You are Tandem's AI-first workflow composer.",
          "Generate a governed automation with explicit nodes, agents, and a final artifact or MCP handoff.",
          `User goal: ${trimmed}`,
          workspaceRoot ? `Workspace root: ${workspaceRoot}` : "",
          allowedMcpServers.length
            ? `Available MCP servers: ${allowedMcpServers.join(", ")}`
            : "No MCP servers are available.",
          "If anything is ambiguous, ask a short clarification question and offer options when possible.",
        ]
          .filter(Boolean)
          .join("\n"),
        schedule: { type: "manual", timezone: "UTC", misfire_policy: { type: "run_once" } },
        plan_source: "automation_composer",
        allowed_mcp_servers: allowedMcpServers,
        workspace_root: workspaceRoot,
        operator_preferences: {
          ...buildDefaultKnowledgeOperatorPreferences(trimmed),
          ...buildKnowledgeRolloutGuidance(trimmed),
        },
      });
    },
    onSuccess: (response, goal) => {
      const nextClarification = summarizeClarifier(response);
      setPlanPreview(response?.plan || null);
      setConversation(response?.conversation || null);
      setClarification(nextClarification);
      setPhase(nextClarification.status === "waiting" ? "clarification" : "draft_ready");
      setInput("");
      setCreatedAutomation(null);
      setCreatedAutomationId("");
      setValidationMessage("");
      if (!draftName.trim()) {
        setDraftName(safeString(response?.plan?.title || goal || "AI-first automation"));
      }
      toast(
        "ok",
        nextClarification.status === "waiting"
          ? "The composer needs one clarification before it can draft the final automation."
          : "The composer drafted an automation plan."
      );
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setValidationMessage(message);
      toast("err", message);
    },
  });

  const messageMutation = useMutation({
    mutationFn: async (message: string) => {
      if (
        !client?.workflowPlans?.chatMessage ||
        !safeString(planPreview?.plan_id || planPreview?.planId)
      ) {
        throw new Error("Start a composer draft first.");
      }
      return client.workflowPlans.chatMessage({
        plan_id: safeString(planPreview?.plan_id || planPreview?.planId),
        message,
      });
    },
    onSuccess: (response) => {
      const nextClarification = summarizeClarifier(response);
      setPlanPreview(response?.plan || null);
      setConversation(response?.conversation || null);
      setClarification(nextClarification);
      setPhase(nextClarification.status === "waiting" ? "clarification" : "draft_ready");
      setInput("");
      setCreatedAutomation(null);
      setCreatedAutomationId("");
      setValidationMessage("");
      toast("ok", "Composer draft updated.");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setValidationMessage(message);
      toast("err", message);
    },
  });

  const resetMutation = useMutation({
    mutationFn: async () => {
      const planId = safeString(planPreview?.plan_id || planPreview?.planId);
      if (!client?.workflowPlans?.chatReset || !planId) return null;
      return client.workflowPlans.chatReset({ plan_id: planId });
    },
    onSuccess: () => {
      resetLocalState();
      toast("ok", "Composer draft reset.");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setValidationMessage(message);
      toast("err", message);
    },
  });

  const agentCreateHeaders = useMemo(() => {
    if (!agentTestMode) return {};
    const agentId = safeString(agentTestAgentId) || DEFAULT_AGENT_ID;
    return {
      "x-tandem-agent-test-mode": "1",
      "x-tandem-request-source": "agent",
      "x-tandem-agent-id": agentId,
    };
  }, [agentTestAgentId, agentTestMode]);

  const createAsAgentRequest = async (payload: any) => {
    const headers = {
      "content-type": "application/json",
      ...(agentCreateHeaders || {}),
    };
    return api("/api/engine/automations/v2", {
      method: "POST",
      headers,
      body: JSON.stringify(payload),
    });
  };

  const runNowAsAgentRequest = async (automationId: string) => {
    const nextId = encodeURIComponent(automationId);
    const headers = {
      "content-type": "application/json",
      ...(agentCreateHeaders || {}),
    };
    return api(`/api/engine/automations/v2/${nextId}/run_now`, {
      method: "POST",
      headers,
    });
  };

  const createMutation = useMutation({
    mutationFn: async (runAfterCreate: boolean) => {
      if (!generatedPayload) {
        throw new Error("Generate a draft first before creating an automation.");
      }
      if (validationErrors.length) {
        throw new Error(validationErrors[0]);
      }
      // If the user picked a non-default execution profile, stamp it
      // onto the generated payload's execution policy before submission
      // so the new automation lands with the chosen default.
      const submissionPayload =
        executionProfile === "strict" ||
        executionProfile === "guided" ||
        executionProfile === "yolo"
          ? {
              ...generatedPayload,
              execution: {
                ...((generatedPayload as Record<string, any>).execution || {}),
                profile: executionProfile,
              },
            }
          : generatedPayload;
      const response = agentTestMode
        ? await createAsAgentRequest(submissionPayload)
        : await client.automationsV2.create(submissionPayload as any);
      const automationId = automationIdFromResponse(response);
      const automation = response?.automation || response?.automation_v2 || response || null;
      return { response, automationId, automation, runAfterCreate };
    },
    onSuccess: async ({ automationId, automation, runAfterCreate }) => {
      setCreatedAutomationId(automationId);
      setCreatedAutomation(automation);
      setPhase("created");
      setValidationMessage("");
      toast(
        "ok",
        runAfterCreate ? "Automation created. Starting the first run..." : "Automation created."
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["studio", "automations"] }),
      ]);
      if (!runAfterCreate) {
        onShowAutomations();
      }
      if (runAfterCreate && automationId) {
        try {
          await runNowMutation.mutateAsync(automationId);
        } catch {
          // The run mutation already surfaced the error toast.
        }
      }
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setValidationMessage(message);
      toast("err", message);
    },
  });

  const runNowMutation = useMutation({
    mutationFn: async (automationId: string) => {
      if (!agentTestMode && !client?.automationsV2?.runNow) {
        throw new Error("This control panel build does not expose automationsV2.runNow.");
      }
      const nextId = safeString(automationId);
      if (!nextId) {
        throw new Error("Create an automation first before running it.");
      }
      return agentTestMode
        ? await runNowAsAgentRequest(nextId)
        : await client.automationsV2.runNow(nextId);
    },
    onSuccess: async () => {
      setValidationMessage("");
      toast("ok", "Run started. Check run history for live status.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["automations"] }),
        queryClient.invalidateQueries({ queryKey: ["studio", "automations"] }),
      ]);
      onShowRuns();
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      setValidationMessage(message);
      toast("err", message);
    },
  });

  const navigationLock = useMemo<NavigationLockState | null>(() => {
    if (startMutation.isPending || messageMutation.isPending) {
      return {
        title: "Generating automation draft",
        message: "Tandem is drafting the automation. Stay on this page until it finishes.",
      };
    }
    if (createMutation.isPending) {
      return {
        title: "Creating automation",
        message: "Tandem is creating the automation. Stay on this page until it finishes.",
      };
    }
    if (runNowMutation.isPending) {
      return {
        title: "Starting the first run",
        message: "Tandem is starting the run. Stay on this page until it finishes.",
      };
    }
    return null;
  }, [
    createMutation.isPending,
    messageMutation.isPending,
    runNowMutation.isPending,
    startMutation.isPending,
  ]);

  useLayoutEffect(() => {
    onNavigationLockChange?.(navigationLock);
    return () => {
      onNavigationLockChange?.(null);
    };
  }, [navigationLock, onNavigationLockChange]);

  const handoffText = useMemo(() => {
    if (!generatedPayload) return "";
    return JSON.stringify(
      {
        phase,
        draftName,
        workspaceRoot,
        prompt: input,
        planId: safeString(planPreview?.plan_id || planPreview?.planId),
        planTitle: safeString(planPreview?.title),
        clarification,
        payload: generatedPayload,
        lastAssistantMessages: normalizedMessages
          .filter((message) => message.role === "assistant" || message.role === "system")
          .slice(-6),
      },
      null,
      2
    );
  }, [
    clarification,
    draftName,
    generatedPayload,
    input,
    normalizedMessages,
    phase,
    planPreview?.planId,
    planPreview?.plan_id,
    planPreview?.title,
    workspaceRoot,
  ]);

  const quickReplies: ChatQuickReply[] = useMemo(() => {
    if (clarification.status !== "waiting") return [];
    return clarification.options.map((option) => ({ id: option.id, label: option.label }));
  }, [clarification]);

  return (
    <div className="flex min-h-0 flex-col gap-4">
      <div className="rounded-2xl border border-slate-700/60 bg-slate-950/40 p-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div className="grid gap-1">
            <div className="text-xs uppercase tracking-[0.24em] text-amber-200/80">
              AI-first workflow composer
            </div>
            <h3 className="text-lg font-semibold text-tcp-text-primary">
              Build automations by talking to Tandem
            </h3>
            <p className="max-w-2xl text-sm text-slate-300">
              Pick a workspace root first, then start with a prompt and Tandem will draft a governed
              automation payload.
            </p>
          </div>
          <div className="flex flex-wrap gap-2 text-xs">
            <span className="tcp-badge-info">phase: {phase}</span>
            <span className="tcp-badge-info">workspace: {workspaceRoot || "not set"}</span>
          </div>
        </div>

        <div className="mt-4 grid gap-3 rounded-xl border border-white/10 bg-black/20 p-3">
          <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
            <label className="grid gap-1">
              <span className="text-xs uppercase tracking-[0.2em] text-slate-400">
                Workspace root
              </span>
              <input
                className="tcp-input h-10"
                value={workspaceRootInput}
                onChange={(event) => {
                  setWorkspaceRootInput(event.target.value);
                  if (validationMessage) setValidationMessage("");
                }}
                placeholder="/workspace/repos/my-repo"
              />
            </label>
            <button
              type="button"
              className="tcp-btn h-10 px-3 text-sm"
              onClick={() => setWorkspaceRootInput(workspaceRootFromHealth)}
              disabled={!workspaceRootFromHealth}
            >
              Use current workspace
            </button>
          </div>
          <div className="text-xs text-slate-400">
            Tandem will scope the generated workflow to this workspace before it drafts or creates
            anything.
          </div>
        </div>

        <div className="mt-4 grid gap-3 rounded-xl border border-white/10 bg-black/25 p-3">
          <label className="flex items-start gap-2">
            <input
              type="checkbox"
              className="mt-1 accent-amber-400"
              checked={agentTestMode}
              onChange={(event) => {
                setAgentTestMode(event.target.checked);
                if (!event.target.checked) {
                  setAgentTestAgentId(DEFAULT_AGENT_ID);
                  setValidationMessage("");
                }
              }}
            />
            <div className="grid gap-1">
              <span className="text-xs uppercase tracking-[0.2em] text-slate-400">
                Testing mode
              </span>
              <span className="text-sm text-slate-200">
                Send create/run requests as a synthetic agent
              </span>
              <span className="text-xs text-slate-400">
                Enable this to exercise agent permission checks in governance and capability review
                flows.
              </span>
            </div>
          </label>
          {agentTestMode ? (
            <label className="grid gap-1">
              <span className="text-xs uppercase tracking-[0.2em] text-slate-400">Agent id</span>
              <input
                className="tcp-input h-9"
                value={agentTestAgentId}
                onChange={(event) => setAgentTestAgentId(event.target.value)}
                placeholder="control-panel-agent"
              />
            </label>
          ) : null}
        </div>
      </div>

      <ChatInterfacePanel
        messages={normalizedMessages}
        emptyText="Pick a workspace root, then start with a prompt and Tandem will draft a governed automation plan."
        inputValue={input}
        inputPlaceholder="Describe the automation you want Tandem to compose."
        sendLabel={phase === "intent_capture" ? "Draft" : "Send"}
        onInputChange={setInput}
        onSend={() => {
          if (clarification.status === "waiting" && safeString(input)) {
            void messageMutation.mutateAsync(input);
            return;
          }
          if (!safeString(planPreview?.plan_id || planPreview?.planId)) {
            void startMutation.mutateAsync(input);
            return;
          }
          void messageMutation.mutateAsync(input);
        }}
        sendDisabled={
          startMutation.isPending ||
          messageMutation.isPending ||
          createMutation.isPending ||
          runNowMutation.isPending ||
          !safeString(input)
        }
        inputDisabled={
          startMutation.isPending ||
          messageMutation.isPending ||
          createMutation.isPending ||
          runNowMutation.isPending
        }
        statusTitle={
          startMutation.isPending || messageMutation.isPending
            ? "Composer is drafting a plan"
            : createMutation.isPending
              ? "Creating automation"
              : runNowMutation.isPending
                ? "Starting the first run"
                : phase === "created"
                  ? "Automation created"
                  : ""
        }
        statusDetail={
          startMutation.isPending || messageMutation.isPending
            ? "The planner is generating or revising the workflow graph."
            : createMutation.isPending
              ? "The generated payload is being submitted to automationsV2.create."
              : runNowMutation.isPending
                ? "The first run is being queued now."
                : phase === "created"
                  ? `Created ${safeString(createdAutomationId || automationIdFromResponse(createdAutomation)) || "automation"}`
                  : ""
        }
        questionTitle={clarification.status === "waiting" ? "Clarification needed" : ""}
        questionText={clarification.status === "waiting" ? clarification.question : ""}
        quickReplies={quickReplies}
        onQuickReply={(option) => {
          void messageMutation.mutateAsync(option.label);
        }}
        questionHint={
          clarification.status === "waiting"
            ? "Pick one of the suggested answers, or type your own response below."
            : "Use the chat input to refine the draft after the first plan is generated."
        }
        autoFocusKey={phase}
      />

      {generatedPayload && phase === "draft_ready" ? (
        <div className="rounded-2xl border border-slate-700/60 bg-slate-950/40 p-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <div className="text-xs uppercase tracking-[0.24em] text-slate-500">Draft ready</div>
              <div className="text-sm font-semibold text-tcp-text-primary">
                Review and create the automation
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                disabled={createMutation.isPending || !!validationErrors.length}
                onClick={() => void createMutation.mutateAsync(false)}
              >
                Create automation
              </button>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                disabled={createMutation.isPending || !!validationErrors.length}
                onClick={() => void createMutation.mutateAsync(true)}
              >
                Create + run now
              </button>
            </div>
          </div>
          <div className="mt-3 grid gap-1">
            <label className="text-xs text-slate-400">Execution profile (saved default)</label>
            <ExecutionProfileToggle
              value={executionProfile}
              clearable
              disabled={createMutation.isPending}
              onChange={(next) => setExecutionProfile(next)}
            />
            <div className="text-xs text-slate-500">
              {executionProfile === "yolo"
                ? "Non-critical validation continues as experimental; spend caps and approvals still enforced."
                : executionProfile === "guided"
                  ? "Non-critical validation becomes warnings; critical failures still block."
                  : executionProfile === "strict"
                    ? "All validators enforced."
                    : "System default is selected. Guided is the fallback when no tenant default is set; use Lenient if recoverable validation or artifact-review checks still block this workflow."}
            </div>
          </div>
        </div>
      ) : null}

      {validationMessage ? (
        <div className="rounded-xl border border-rose-500/30 bg-rose-950/20 p-3 text-sm text-rose-100">
          {validationMessage}
        </div>
      ) : null}

      <details className="rounded-2xl border border-slate-700/60 bg-slate-950/40 p-4">
        <summary className="cursor-pointer list-none text-sm font-medium text-slate-200">
          Advanced preview and handoff
        </summary>
        <div className="mt-4 grid gap-4">
          <div className="rounded-xl border border-slate-700/50 bg-slate-900/40 p-4">
            <div className="text-xs uppercase tracking-[0.24em] text-slate-500">
              Suggested prompts
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                onClick={() =>
                  setInput(
                    'Build a governed automation named "Todo digest + notify" that scans src/ and docs/ for TODO/FIXME items, writes docs/todo_digest.md, then sends a short Slack summary with the report path.'
                  )
                }
              >
                TODO digest + notify
              </button>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                onClick={() =>
                  setInput(
                    "Create a workflow that reads key repo files, summarizes risks in docs/repo_audit.md, and posts a final MCP notification when the report is ready."
                  )
                }
              >
                Repo audit
              </button>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                onClick={() =>
                  setInput(
                    "Draft an automation that collects release-ready changes, writes release notes, and sends a final message to the release channel."
                  )
                }
              >
                Release notes
              </button>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                onClick={() => {
                  if (!safeString(planPreview?.plan_id || planPreview?.planId)) {
                    resetLocalState();
                    toast("ok", "Composer draft reset.");
                    return;
                  }
                  void resetMutation.mutateAsync();
                }}
                disabled={
                  resetMutation.isPending ||
                  startMutation.isPending ||
                  messageMutation.isPending ||
                  createMutation.isPending ||
                  runNowMutation.isPending
                }
              >
                Reset draft
              </button>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                onClick={() => setPayloadMode((current) => (current === "json" ? "yaml" : "json"))}
              >
                Show {payloadMode === "json" ? "YAML" : "JSON"}
              </button>
            </div>
          </div>

          <div className="rounded-xl border border-slate-700/50 bg-slate-900/40 p-4">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-xs uppercase tracking-[0.24em] text-slate-500">
                  Runbook replay
                </div>
                <h4 className="text-sm font-semibold text-tcp-text-primary">What the automation will do</h4>
              </div>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                onClick={() =>
                  void copyText(handoffText).then(
                    (ok) => ok && toast("ok", "Handoff export copied.")
                  )
                }
                disabled={!generatedPayload}
              >
                Handoff export
              </button>
            </div>
            <div className="mt-3 grid gap-2">
              {(Array.isArray(planPreview?.steps) ? planPreview.steps : []).length ? (
                (planPreview.steps || []).map((step: any, index: number) => {
                  const stepId = safeString(step?.step_id || step?.stepId || `step-${index + 1}`);
                  const objective = safeString(step?.objective || `Step ${index + 1}`);
                  const role = safeString(step?.agent_role || step?.agentRole || "worker");
                  const dependsOn = normalizeStringArray(step?.depends_on || step?.dependsOn);
                  return (
                    <div key={stepId} className="rounded-xl border border-white/10 bg-black/20 p-3">
                      <div className="flex items-center justify-between gap-2">
                        <div className="text-sm font-medium text-tcp-text-primary">
                          {index + 1}. {objective}
                        </div>
                        <span className="tcp-badge-info">{titleCase(role)}</span>
                      </div>
                      <div className="mt-1 text-xs text-slate-400">
                        {dependsOn.length
                          ? `Depends on ${dependsOn.join(", ")}`
                          : "First step in the chain"}
                      </div>
                      <div className="mt-2 text-xs text-slate-300">
                        Node: <code>{stepId}</code>
                      </div>
                    </div>
                  );
                })
              ) : (
                <div className="rounded-xl border border-dashed border-slate-700/70 bg-slate-950/20 p-4 text-sm text-slate-400">
                  No draft yet. Start from the prompt and the runbook will appear here.
                </div>
              )}
            </div>
          </div>

          <div className="rounded-xl border border-slate-700/50 bg-slate-900/40 p-4">
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="text-xs uppercase tracking-[0.24em] text-slate-500">
                  Payload preview
                </div>
                <h4 className="text-sm font-semibold text-tcp-text-primary">automationV2.create body</h4>
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  className={`tcp-btn h-8 px-2 text-xs ${
                    payloadMode === "json"
                      ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                      : ""
                  }`}
                  onClick={() => setPayloadMode("json")}
                >
                  JSON
                </button>
                <button
                  type="button"
                  className={`tcp-btn h-8 px-2 text-xs ${
                    payloadMode === "yaml"
                      ? "border-amber-400/60 bg-amber-400/10 text-amber-300"
                      : ""
                  }`}
                  onClick={() => setPayloadMode("yaml")}
                >
                  YAML
                </button>
              </div>
            </div>
            <div className="mt-3 flex flex-wrap gap-2 text-xs">
              <span className="tcp-badge-info">apply preview</span>
              <span className="tcp-badge-info">validate before create</span>
              <span className="tcp-badge-info">runNow after create</span>
            </div>
            <pre className="mt-3 max-h-[20rem] overflow-auto rounded-xl border border-white/10 bg-black/40 p-3 tcp-text-caption leading-5 text-slate-200">
              {previewText || "Generate a draft to see the payload."}
            </pre>
            <div className="mt-3 flex flex-wrap gap-2">
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                disabled={!generatedPayload}
                onClick={() =>
                  void copyText(previewText).then(
                    (ok) => ok && toast("ok", `${previewLabel} copied.`)
                  )
                }
              >
                Copy {previewLabel}
              </button>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                disabled={!generatedPayload}
                onClick={() => {
                  if (!validationErrors.length) {
                    setValidationMessage("Payload looks ready for create.");
                    toast("ok", "Validation passed.");
                    return;
                  }
                  setValidationMessage(validationErrors[0]);
                  toast("warn", validationErrors[0]);
                }}
              >
                Validate
              </button>
              <button
                type="button"
                className="tcp-btn h-9 px-3 text-sm"
                disabled={!generatedPayload}
                onClick={() => {
                  setPhase("draft_ready");
                  toast("ok", "Preview applied.");
                }}
              >
                Apply preview
              </button>
            </div>
            {validationErrors.length ? (
              <div className="mt-3 rounded-xl border border-rose-500/30 bg-rose-950/20 p-3 text-sm text-rose-100">
                {validationErrors[0]}
              </div>
            ) : null}
          </div>

          <div className="rounded-xl border border-slate-700/50 bg-slate-900/40 p-4">
            <div className="text-xs uppercase tracking-[0.24em] text-slate-500">
              Docs-aware context
            </div>
            <div className="mt-2 text-sm font-semibold text-tcp-text-primary">
              Resources for agents and developers
            </div>
            <div className="mt-3 grid gap-2 text-sm text-slate-300">
              <a
                className="text-amber-300 hover:text-amber-200"
                href="/docs/automation-composer-workflows/"
              >
                Build an automation with the AI assistant
              </a>
              <a
                className="text-amber-300 hover:text-amber-200"
                href="/docs/automation-examples-for-teams/"
              >
                Automation Examples For Teams
              </a>
              <a className="text-amber-300 hover:text-amber-200" href="/docs/sdk/typescript/">
                TypeScript SDK automation examples
              </a>
              <a className="text-amber-300 hover:text-amber-200" href="/docs/sdk/python/">
                Python SDK automation examples
              </a>
            </div>
            <div className="mt-3 text-xs text-slate-400">
              The production agent path can pre-connect Tandem Docs MCP so clarifications and schema
              lookups stay grounded in the canonical docs.
            </div>
          </div>

          {createdAutomationId ? (
            <div className="rounded-xl border border-slate-700/50 bg-slate-900/40 p-4">
              <div className="text-xs uppercase tracking-[0.24em] text-slate-500">
                Created automation
              </div>
              <div className="mt-2 grid gap-2">
                <div className="text-sm text-tcp-text-primary">
                  Automation ID: <code>{createdAutomationId}</code>
                </div>
                <div className="text-sm text-slate-300">
                  {safeString(
                    createdAutomation?.name || planPreview?.title || draftName || "Automation"
                  )}
                </div>
                <div className="flex flex-wrap gap-2 text-xs">
                  <span className="tcp-badge-ok">
                    latest run: {latestRun ? safeString(latestRun.status || "queued") : "waiting"}
                  </span>
                  {latestRun?.run_id ? (
                    <span className="tcp-badge-info">run {safeString(latestRun.run_id)}</span>
                  ) : null}
                </div>
                {latestRun?.run_id ? (
                  <div className="rounded-xl border border-white/10 bg-black/20 p-3 text-sm text-slate-300">
                    The run is visible in the standard run history. Open the running tasks tab to
                    follow it end to end.
                  </div>
                ) : null}
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className="tcp-btn h-9 px-3 text-sm"
                    onClick={onShowAutomations}
                  >
                    Open automations
                  </button>
                  <button type="button" className="tcp-btn h-9 px-3 text-sm" onClick={onShowRuns}>
                    View runs
                  </button>
                  <button
                    type="button"
                    className="tcp-btn h-9 px-3 text-sm"
                    disabled={
                      !createdAutomationId ||
                      runNowMutation.isPending ||
                      startMutation.isPending ||
                      messageMutation.isPending ||
                      createMutation.isPending
                    }
                    onClick={() => void runNowMutation.mutateAsync(createdAutomationId)}
                  >
                    Run now
                  </button>
                </div>
              </div>
            </div>
          ) : null}
        </div>
      </details>
    </div>
  );
}
