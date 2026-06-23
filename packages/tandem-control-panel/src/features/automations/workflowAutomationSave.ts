type WorkflowAutomationSaveArgs = {
  draft: any;
  client: any;
  automationsV2: any[];
  helperFns: any;
};

export async function updateWorkflowAutomationDraft({
  draft,
  client,
  automationsV2,
  helperFns,
}: WorkflowAutomationSaveArgs) {
  const {
    validateModelInput,
    validatePlannerModelInput,
    validateWorkspaceRootInput,
    workflowEditToOperatorPreferences,
    compileWorkflowModelPolicy,
    cloneJsonValue,
    compileWorkflowToolAllowlist,
    parseConnectorBindingsJson,
    workflowNodeModelPolicyWithOverride,
    deriveConnectorBindingResolutionFromPlanPackage,
    workflowEditToSchedule,
    uniqueStrings,
  } = helperFns;
  const name = String(draft.name || "").trim();
  const description = String(draft.description || "").trim();
  const workspaceRoot = String(draft.workspaceRoot || "").trim();
  const modelError = validateModelInput(draft.modelProvider, draft.modelId);
  const plannerModelError = validatePlannerModelInput(draft.plannerModelProvider, draft.plannerModelId);
  const workspaceError = validateWorkspaceRootInput(workspaceRoot);
  if (!name) throw new Error("Automation name is required.");
  if (workspaceError) throw new Error(workspaceError);
  if (modelError) throw new Error(modelError);
  if (plannerModelError) throw new Error(plannerModelError);
  if (draft.scheduleKind === "cron" && !String(draft.cronExpression || "").trim()) {
    throw new Error("Cron expression is required.");
  }
  if (
    draft.scheduleKind === "interval" &&
    (!Number.isFinite(Number(draft.intervalSeconds)) || Number(draft.intervalSeconds) <= 0)
  ) {
    throw new Error("Interval seconds must be greater than zero.");
  }
  const operatorPreferences = workflowEditToOperatorPreferences(draft);
  const modelPolicy = compileWorkflowModelPolicy(operatorPreferences);
  const baseModelPolicy = modelPolicy ? (cloneJsonValue(modelPolicy) as Record<string, any>) : null;
  const selectedMcpServers = draft.selectedMcpServers
    .map((row: any) => String(row || "").trim())
    .filter(Boolean);
  const toolAllowlist = compileWorkflowToolAllowlist(
    selectedMcpServers,
    draft.toolAccessMode,
    draft.customToolsText,
    draft.selectedMcpTools,
    draft.mcpOtherAllowedTools
  );
  const connectorBindings = parseConnectorBindingsJson(draft.connectorBindingsJson);
  const sharedContextPackIds = uniqueStrings(
    String(draft.sharedContextPackIdsText || "")
      .split(/[\n,]/g)
      .map((value: string) => String(value || "").trim())
      .filter(Boolean)
  );
  const sharedContextBindings = sharedContextPackIds.map((packId: string) => ({
    pack_id: packId,
    required: true,
  }));
  const stepModelPolicies = new Map<string, Record<string, any> | null>();
  for (const node of draft.nodes) {
    const nodeAgentId = String(node.agentId || "").trim();
    if (!nodeAgentId) continue;
    const nodeModelProvider = String(node.modelProvider || "").trim();
    const nodeModelId = String(node.modelId || "").trim();
    const nodeModelError = validateModelInput(nodeModelProvider, nodeModelId);
    if (nodeModelError) {
      throw new Error(`${node.title || node.nodeId || nodeAgentId}: ${nodeModelError}`);
    }
    stepModelPolicies.set(
      nodeAgentId,
      workflowNodeModelPolicyWithOverride(baseModelPolicy, nodeModelProvider, nodeModelId)
    );
  }
  const nextScopeSnapshot = draft.scopeSnapshot ? cloneJsonValue(draft.scopeSnapshot) : null;
  if (nextScopeSnapshot && typeof nextScopeSnapshot === "object") {
    nextScopeSnapshot.connector_bindings = connectorBindings;
    nextScopeSnapshot.connector_binding_resolution = deriveConnectorBindingResolutionFromPlanPackage(
      nextScopeSnapshot,
      connectorBindings
    );
  }
  let existing = automationsV2.find(
    (row: any) =>
      String(row?.automation_id || row?.automationId || row?.id || "").trim() === draft.automationId
  );
  const summaryMissingRunnableShape =
    !Array.isArray(existing?.agents) ||
    existing.agents.length === 0 ||
    !Array.isArray(existing?.flow?.nodes);
  if (summaryMissingRunnableShape && client?.automationsV2?.get) {
    try {
      const response = await client.automationsV2.get(draft.automationId);
      if (response?.automation && typeof response.automation === "object") {
        existing = response.automation;
      }
    } catch {
      // Keep the summary row and avoid clearing omitted fields below.
    }
  }
  const agents =
    Array.isArray(existing?.agents) && existing.agents.length > 0
      ? existing.agents.map((agent: any) => {
          const agentId = String(agent?.agent_id || agent?.agentId || "").trim();
          const nextModelPolicy = stepModelPolicies.has(agentId)
            ? stepModelPolicies.get(agentId)
            : agent?.model_policy || agent?.modelPolicy || modelPolicy;
          return {
            ...agent,
            model_policy: nextModelPolicy ? cloneJsonValue(nextModelPolicy) : null,
            modelPolicy: undefined,
            tool_policy: {
              ...(agent?.tool_policy || {}),
              allowlist: toolAllowlist,
              denylist: Array.isArray(agent?.tool_policy?.denylist) ? agent.tool_policy.denylist : [],
            },
            mcp_policy: {
              ...(agent?.mcp_policy || {}),
              allowed_servers: selectedMcpServers,
              allowed_tools:
                draft.toolAccessMode === "all"
                  ? null
                  : Array.isArray(draft.selectedMcpTools)
                    ? [...draft.mcpOtherAllowedTools, ...draft.selectedMcpTools]
                    : draft.mcpOtherAllowedTools,
            },
          };
        })
      : null;
  const flowNodes = Array.isArray(existing?.flow?.nodes)
    ? existing.flow.nodes.map((node: any, index: number) => {
        const nodeId = String(node?.node_id || node?.nodeId || node?.id || `node-${index}`).trim();
        const draftNode = draft.nodes.find((row: any) => row.nodeId === nodeId);
        const nodeMcpAllowedTools =
          draftNode?.toolAccessMode === "custom"
            ? Array.isArray(draftNode.mcpAllowedTools)
              ? [...(draftNode.mcpOtherAllowedTools || []), ...draftNode.mcpAllowedTools]
              : draftNode.mcpOtherAllowedTools || []
            : [];
        const nodeMetadata =
          node?.metadata && typeof node.metadata === "object" ? cloneJsonValue(node.metadata) : {};
        const approvalMetadata =
          nodeMetadata?.approval && typeof nodeMetadata.approval === "object"
            ? { ...nodeMetadata.approval }
            : {};
        if (draftNode?.approvalOverride === "skip") {
          approvalMetadata.skip_approval = true;
          delete approvalMetadata.auto_approve_when;
          delete approvalMetadata.autoApproveWhen;
        } else if (draftNode?.approvalOverride === "auto") {
          const approvalCondition = String(draftNode.approvalCondition || "").trim();
          delete approvalMetadata.skip_approval;
          delete approvalMetadata.skipApproval;
          if (approvalCondition) {
            approvalMetadata.auto_approve_when = approvalCondition;
          } else {
            delete approvalMetadata.auto_approve_when;
            delete approvalMetadata.autoApproveWhen;
          }
        } else if (draftNode) {
          delete approvalMetadata.skip_approval;
          delete approvalMetadata.skipApproval;
          delete approvalMetadata.auto_approve_when;
          delete approvalMetadata.autoApproveWhen;
        }
        const nextMetadata = { ...nodeMetadata };
        if (Object.keys(approvalMetadata).length > 0) {
          nextMetadata.approval = approvalMetadata;
        } else {
          delete nextMetadata.approval;
        }
        return draftNode
          ? {
              ...node,
              objective: String(draftNode.objective || "").trim(),
              metadata: nextMetadata,
              ...(draftNode.approvalOverride === "skip" ? { gate: null } : {}),
              tool_policy:
                draftNode.toolAccessMode === "custom"
                  ? {
                      allowlist: [
                        ...(draftNode.toolAllowlist || []),
                        ...(draftNode.mcpAllowedTools === null
                          ? (draftNode.mcpAllowedServers || []).map(
                              (server: string) =>
                                `mcp.${
                                  String(server || "")
                                    .trim()
                                    .toLowerCase()
                                    .replace(/[^a-z0-9]+/g, "_")
                                    .replace(/^_+|_+$/g, "") || "mcp"
                                }.*`
                            )
                          : nodeMcpAllowedTools),
                      ]
                        .map((entry: string) => String(entry || "").trim())
                        .filter(Boolean),
                      denylist: (draftNode.toolDenylist || [])
                        .map((entry: string) => String(entry || "").trim())
                        .filter(Boolean),
                    }
                  : undefined,
              toolPolicy: undefined,
              mcp_policy:
                draftNode.toolAccessMode === "custom"
                  ? {
                      ...(node?.mcp_policy || {}),
                      allowed_servers: (draftNode.mcpAllowedServers || [])
                        .map((entry: string) => String(entry || "").trim())
                        .filter(Boolean),
                      allowed_tools:
                        draftNode.mcpAllowedTools === null
                          ? null
                          : nodeMcpAllowedTools
                              .map((entry: string) => String(entry || "").trim())
                              .filter(Boolean),
                      allowed_connections: draftNode.mcpAllowedConnections || [],
                    }
                  : undefined,
              mcpPolicy: undefined,
            }
          : node;
      })
    : [];
  const existingMetadata =
    existing?.metadata && typeof existing.metadata === "object" ? existing.metadata : {};
  const nextPlanPackage = nextScopeSnapshot
    ? {
        ...(cloneJsonValue(existingMetadata?.plan_package) || {}),
        ...nextScopeSnapshot,
      }
    : existingMetadata?.plan_package;
  const sharedContextProjectKey = String(
    nextScopeSnapshot?.project_key ||
      nextScopeSnapshot?.projectKey ||
      existingMetadata?.shared_context_project_key ||
      existingMetadata?.sharedContextProjectKey ||
      ""
  ).trim();
  if (nextScopeSnapshot && typeof nextScopeSnapshot === "object") {
    nextScopeSnapshot.shared_context_pack_ids = sharedContextPackIds;
    nextScopeSnapshot.shared_context_bindings = sharedContextBindings;
    if (sharedContextProjectKey) {
      nextScopeSnapshot.shared_context_project_key = sharedContextProjectKey;
    }
    nextScopeSnapshot.shared_context_workspace_root = workspaceRoot;
  }
  const nextPlanPackageBundle =
    nextScopeSnapshot && existingMetadata?.plan_package_bundle
      ? {
          ...cloneJsonValue(existingMetadata.plan_package_bundle),
          scope_snapshot: nextScopeSnapshot,
        }
      : existingMetadata?.plan_package_bundle;
  return client.automationsV2.update(draft.automationId, {
    name,
    description: description || null,
    schedule: workflowEditToSchedule(draft),
    workspace_root: workspaceRoot,
    execution: {
      ...(existing?.execution || {}),
      max_parallel_agents:
        draft.executionMode === "single"
          ? 1
          : Math.max(
              2,
              Math.min(16, Number.parseInt(String(draft.maxParallelAgents || "4"), 10) || 4)
            ),
      ...(draft.executionProfile === "strict" ||
      draft.executionProfile === "guided" ||
      draft.executionProfile === "yolo"
        ? { profile: draft.executionProfile }
        : { profile: null }),
    },
    flow: existing?.flow
      ? {
          ...existing.flow,
          nodes: flowNodes,
        }
      : existing?.flow,
    ...(agents ? { agents } : {}),
    ...(draft.handoffConfig != null ? { handoff_config: draft.handoffConfig } : {}),
    ...(Array.isArray(draft.watchConditions) && draft.watchConditions.length > 0
      ? { watch_conditions: draft.watchConditions }
      : {}),
    ...(draft.scopePolicy != null ? { scope_policy: draft.scopePolicy } : {}),
    metadata: {
      ...existingMetadata,
      workspace_root: workspaceRoot,
      operator_preferences: operatorPreferences,
      allowed_mcp_servers: selectedMcpServers,
      ...(nextPlanPackage ? { plan_package: nextPlanPackage } : {}),
      ...(nextPlanPackageBundle ? { plan_package: nextPlanPackageBundle } : {}),
      shared_context_pack_ids: sharedContextPackIds,
      shared_context_bindings: sharedContextBindings,
      ...(sharedContextProjectKey ? { shared_context_project_key: sharedContextProjectKey } : {}),
      shared_context_workspace_root: workspaceRoot,
    },
  });
}
