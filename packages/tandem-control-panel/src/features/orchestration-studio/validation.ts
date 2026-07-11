import type {
  OrchestrationNodeSpec,
  OrchestrationSpec,
  OrchestrationValidationIssue,
  OrchestrationValidationReport,
  OrchestrationValueBinding,
  OrchestrationWaitSpec,
  OrchestrationWaitTimeoutPolicy,
} from "@frumu/tandem-client";
import { analyzeGraph } from "./graph";

function issue(
  code: string,
  message: string,
  refs: Pick<OrchestrationValidationIssue, "node_id" | "edge_id"> = {}
): OrchestrationValidationIssue {
  return { code, message, ...refs };
}

function invalidBinding(binding: OrchestrationValueBinding): boolean {
  return binding.source === "literal"
    ? binding.value === null
    : binding.node_id.trim().length === 0;
}

function invalidTimeout(timeout: OrchestrationWaitTimeoutPolicy): boolean {
  return (
    !Number.isSafeInteger(timeout.expires_after_ms) ||
    timeout.expires_after_ms <= 0 ||
    (timeout.on_timeout === "escalate" && !timeout.escalate_to?.trim()) ||
    (timeout.remind_every_ms !== undefined &&
      (!Number.isSafeInteger(timeout.remind_every_ms) || timeout.remind_every_ms <= 0))
  );
}

function waitBindings(wait: OrchestrationWaitSpec): OrchestrationValueBinding[] {
  if (wait.kind === "timer") return wait.wake_at ? [wait.wake_at] : [];
  if (wait.kind === "webhook") return [wait.correlation.value];
  if (wait.kind === "external_condition") return [wait.condition_key];
  return [];
}

export function validateWait(
  wait: OrchestrationWaitSpec,
  nodeId?: string
): OrchestrationValidationIssue[] {
  const issues: OrchestrationValidationIssue[] = [];
  const add = (code: string, message: string): void => {
    issues.push(issue(code, message, nodeId ? { node_id: nodeId } : {}));
  };
  if (wait.kind === "timer") {
    if (Number(wait.delay_ms !== undefined) + Number(wait.wake_at !== undefined) !== 1) {
      add("timer_wake_conflict", "Timer waits require exactly one of delay_ms or wake_at");
    }
    if (
      wait.delay_ms !== undefined &&
      (!Number.isSafeInteger(wait.delay_ms) || wait.delay_ms <= 0)
    ) {
      add("timer_delay_invalid", "Timer delay_ms must be a positive integer");
    }
    if (wait.wake_at?.source === "literal") {
      const value = wait.wake_at.value;
      if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0) {
        add(
          "timer_wake_at_invalid",
          "Literal timer wake_at must be a positive millisecond timestamp"
        );
      }
    }
    if (wait.timeout && invalidTimeout(wait.timeout)) {
      add("wait_timeout_invalid", "Wait timeout policy is invalid");
    }
  } else if (wait.kind === "approval") {
    const normalized = wait.decisions.map((decision) =>
      decision.trim().replace(/[A-Z]/g, (character) => character.toLowerCase())
    );
    if (
      !normalized.length ||
      normalized.some((decision) => !decision) ||
      new Set(normalized).size !== normalized.length
    ) {
      add("approval_decisions_invalid", "Approval waits require unique, non-empty decisions");
    }
    if (
      wait.expires_after_ms !== undefined &&
      (!Number.isSafeInteger(wait.expires_after_ms) || wait.expires_after_ms <= 0)
    ) {
      add("approval_expiry_invalid", "Approval expires_after_ms must be a positive integer");
    }
    if (wait.expires_after_ms !== undefined && wait.timeout !== undefined) {
      add(
        "approval_timeout_conflict",
        "Approval waits cannot define both expires_after_ms and timeout"
      );
    }
    if (wait.timeout && invalidTimeout(wait.timeout)) {
      add("wait_timeout_invalid", "Wait timeout policy is invalid");
    }
  } else if (wait.kind === "webhook") {
    if (!wait.trigger_id.trim())
      add("webhook_trigger_invalid", "Webhook waits require a trigger_id");
    if (invalidBinding(wait.correlation.value)) {
      add("webhook_correlation_invalid", "Webhook waits require a typed correlation constraint");
    }
    if (invalidTimeout(wait.timeout)) add("wait_timeout_invalid", "Wait timeout policy is invalid");
  } else {
    if (invalidBinding(wait.condition_key)) {
      add("external_condition_invalid", "External-condition waits require a typed condition key");
    }
    if (invalidTimeout(wait.timeout)) add("wait_timeout_invalid", "Wait timeout policy is invalid");
  }
  return issues;
}

function validateNode(
  node: OrchestrationNodeSpec,
  spec: OrchestrationSpec
): OrchestrationValidationIssue[] {
  const issues: OrchestrationValidationIssue[] = [];
  if (!node.node_id.trim()) issues.push(issue("empty_node_id", "Node IDs cannot be empty"));
  if (node.kind === "workflow") {
    if (!node.automation_id.trim()) {
      issues.push(
        issue("missing_automation_id", "Workflow nodes must reference an automation", {
          node_id: node.node_id,
        })
      );
    }
    if (spec.status === "published" && !node.pinned_definition_hash?.trim()) {
      issues.push(
        issue("unpinned_workflow", "Published workflow nodes require a definition hash", {
          node_id: node.node_id,
        })
      );
    }
    const keys = node.allowed_transition_keys ?? [];
    if (keys.some((key) => !key.trim()) || new Set(keys).size !== keys.length) {
      issues.push(
        issue(
          "invalid_allowed_transition_key",
          "Workflow transition keys must be non-empty and unique",
          { node_id: node.node_id }
        )
      );
    }
  } else if (node.kind === "wait") {
    issues.push(...validateWait(node.wait, node.node_id));
    for (const binding of waitBindings(node.wait)) {
      if (
        binding.source === "node_output" &&
        binding.node_id &&
        !spec.nodes.some((candidate) => candidate.node_id === binding.node_id)
      ) {
        issues.push(
          issue("wait_binding_unknown_node", "Wait binding references an unknown node", {
            node_id: node.node_id,
          })
        );
      }
      if (
        binding.source === "node_output" &&
        binding.json_pointer &&
        !binding.json_pointer.startsWith("/")
      ) {
        issues.push(
          issue(
            "wait_binding_invalid_json_pointer",
            "Wait binding json_pointer must start with '/'",
            { node_id: node.node_id }
          )
        );
      }
    }
  }
  return issues;
}

/** Fast client validation. The server remains authoritative at publish time. */
export function validateOrchestrationDraft(spec: OrchestrationSpec): OrchestrationValidationReport {
  const issues: OrchestrationValidationIssue[] = [];
  if (spec.schema_version !== 1)
    issues.push(issue("unsupported_schema_version", "Only schema version 1 is supported"));
  if (!Number.isSafeInteger(spec.version) || spec.version < 0) {
    issues.push(issue("invalid_version", "Draft version is 0 and published versions start at 1"));
  }
  if (spec.status === "draft" && spec.version !== 0) {
    issues.push(issue("invalid_draft_version", "Mutable drafts must use version 0"));
  }
  if (spec.status === "published" && spec.published_at_ms === undefined) {
    issues.push(issue("missing_published_at", "Published versions require published_at_ms"));
  }
  if (!Number.isSafeInteger(spec.goal_policy.max_hops) || spec.goal_policy.max_hops <= 0) {
    issues.push(issue("invalid_max_hops", "max_hops must be greater than zero"));
  }
  const nodeIds = new Set<string>();
  for (const node of spec.nodes) {
    if (nodeIds.has(node.node_id))
      issues.push(issue("duplicate_node_id", "Node IDs must be unique", { node_id: node.node_id }));
    nodeIds.add(node.node_id);
    issues.push(...validateNode(node, spec));
  }
  if (!nodeIds.has(spec.root_node_id)) {
    issues.push(
      issue("missing_root", "The root must reference an existing node", {
        node_id: spec.root_node_id,
      })
    );
  }

  const edgeIds = new Set<string>();
  const transitionKeys = new Set<string>();
  const outgoingCounts = new Map<string, number>();
  for (const edge of spec.edges) {
    if (edgeIds.has(edge.edge_id))
      issues.push(issue("duplicate_edge_id", "Edge IDs must be unique", { edge_id: edge.edge_id }));
    edgeIds.add(edge.edge_id);
    const source = spec.nodes.find((node) => node.node_id === edge.from_node_id);
    const target = spec.nodes.find((node) => node.node_id === edge.to_node_id);
    if (!source || !target) {
      issues.push(
        issue("unknown_edge_node", "Edges must reference existing source and target nodes", {
          edge_id: edge.edge_id,
        })
      );
      continue;
    }
    outgoingCounts.set(source.node_id, (outgoingCounts.get(source.node_id) ?? 0) + 1);
    if (!edge.transition_key.trim())
      issues.push(
        issue("empty_transition_key", "Transition keys cannot be empty", { edge_id: edge.edge_id })
      );
    const transitionIdentity = `${edge.from_node_id}\u0000${edge.transition_key}`;
    if (transitionKeys.has(transitionIdentity)) {
      issues.push(
        issue("duplicate_transition_key", "Transition keys must be unique for each source node", {
          node_id: source.node_id,
          edge_id: edge.edge_id,
        })
      );
    }
    transitionKeys.add(transitionIdentity);
    if (source.kind === "terminal") {
      issues.push(
        issue("terminal_has_outgoing_edge", "Terminal nodes cannot have outgoing transitions", {
          node_id: source.node_id,
          edge_id: edge.edge_id,
        })
      );
    }
    if (
      source.kind === "workflow" &&
      !(source.allowed_transition_keys ?? []).includes(edge.transition_key)
    ) {
      issues.push(
        issue(
          "unknown_transition_key",
          "Edge transition key is not declared by its workflow node",
          { node_id: source.node_id, edge_id: edge.edge_id }
        )
      );
    }
  }
  for (const node of spec.nodes) {
    if (node.kind !== "terminal" && !(outgoingCounts.get(node.node_id) ?? 0)) {
      issues.push(
        issue("missing_outgoing_transition", "Nonterminal nodes require an outgoing transition", {
          node_id: node.node_id,
        })
      );
    }
  }

  const analysis = analyzeGraph(spec);
  for (const nodeId of analysis.orphanNodeIds) {
    issues.push(
      issue("orphan_node", "Non-root node has no incoming transitions", { node_id: nodeId })
    );
  }
  for (const node of spec.nodes) {
    if (!analysis.reachableNodeIds.has(node.node_id)) {
      issues.push(
        issue("unreachable_node", "Every node must be reachable from the root", {
          node_id: node.node_id,
        })
      );
    }
  }
  if (!analysis.terminalNodeIds.length)
    issues.push(issue("missing_terminal", "The graph requires a terminal node"));
  for (const nodeId of analysis.reachableNodeIds) {
    if (!analysis.canReachTerminalNodeIds.has(nodeId)) {
      issues.push(
        issue("no_terminal_path", "Every reachable node must have a path to a terminal", {
          node_id: nodeId,
        })
      );
    }
  }
  for (const component of analysis.loopComponents) {
    if (component.every((nodeId) => !analysis.canReachTerminalNodeIds.has(nodeId))) {
      issues.push(
        issue("unbounded_cycle", "Cycle has no path to a terminal", { node_id: component[0] })
      );
    }
  }
  return { valid: issues.length === 0, issues };
}

export function issuesForNode(
  report: OrchestrationValidationReport,
  nodeId: string
): OrchestrationValidationIssue[] {
  return report.issues.filter((entry) => entry.node_id === nodeId);
}

export function issuesForEdge(
  report: OrchestrationValidationReport,
  edgeId: string
): OrchestrationValidationIssue[] {
  return report.issues.filter((entry) => entry.edge_id === edgeId);
}
