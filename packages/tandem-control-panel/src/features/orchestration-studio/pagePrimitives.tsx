import type {
  OrchestrationNodeSpec,
  OrchestrationSpec,
  OrchestrationWaitSpec,
} from "@frumu/tandem-client";
import { Icon } from "../../ui/Icon";

const emptyPosition = { x: 96, y: 96 };

export function orchestrationEditorId(prefix: string) {
  const suffix = globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random()}`;
  return `${prefix}-${suffix}`;
}

export function automationId(row: any) {
  return String(row?.automation_id || row?.automationId || row?.id || "").trim();
}

export function automationName(row: any) {
  return String(row?.name || row?.title || automationId(row) || "Untitled workflow").trim();
}

export function formatOrchestrationTime(value?: number | null) {
  if (!value) return "No activity";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(value)
  );
}

export function createOrchestrationNode(
  kind: string,
  position = emptyPosition
): OrchestrationNodeSpec {
  if (kind.startsWith("workflow:")) {
    const workflowId = kind.slice("workflow:".length);
    return {
      node_id: orchestrationEditorId("workflow"),
      name: workflowId,
      position,
      kind: "workflow",
      automation_id: workflowId,
      allowed_transition_keys: [],
    };
  }
  if (kind.startsWith("wait:")) {
    const waitKind = kind.slice("wait:".length);
    const wait: OrchestrationWaitSpec =
      waitKind === "approval"
        ? { kind: "approval", decisions: ["approve", "reject"] }
        : waitKind === "webhook"
          ? {
              kind: "webhook",
              trigger_id: "",
              correlation: {
                field: "provider_event_id",
                value: { source: "literal", value: "" },
              },
              timeout: { expires_after_ms: 86_400_000, on_timeout: "cancel" },
            }
          : waitKind === "external_condition"
            ? {
                kind: "external_condition",
                condition_key: { source: "literal", value: "" },
                timeout: { expires_after_ms: 86_400_000, on_timeout: "cancel" },
              }
            : { kind: "timer", delay_ms: 3_600_000 };
    return {
      node_id: orchestrationEditorId("wait"),
      name: `${waitKind.replace("_", " ")} wait`,
      position,
      kind: "wait",
      wait,
    };
  }
  const outcome = kind.slice("terminal:".length) as "complete" | "pause" | "fail";
  return {
    node_id: orchestrationEditorId("terminal"),
    name: `${outcome[0].toUpperCase()}${outcome.slice(1)}`,
    position,
    kind: "terminal",
    outcome,
  };
}

export function orchestrationGraphSnapshot(spec: OrchestrationSpec) {
  return JSON.stringify({
    name: spec.name,
    description: spec.description,
    root_node_id: spec.root_node_id,
    nodes: spec.nodes,
    edges: spec.edges,
    goal_policy: spec.goal_policy,
    metadata: spec.metadata,
  });
}

export function sameStringSet(current: Set<string>, values: string[]) {
  return current.size === values.length && values.every((value) => current.has(value));
}

export function synchronizeWorkflowTransitionKeys(spec: OrchestrationSpec): OrchestrationSpec {
  return {
    ...spec,
    nodes: spec.nodes.map((node) => {
      if (node.kind !== "workflow") return node;
      return {
        ...node,
        allowed_transition_keys: Array.from(
          new Set(
            spec.edges
              .filter((edge) => edge.from_node_id === node.node_id)
              .map((edge) => edge.transition_key)
              .filter(Boolean)
          )
        ),
      };
    }),
  };
}

export function compareOrchestrationSpecs(
  current: OrchestrationSpec,
  baseline: OrchestrationSpec | null
) {
  if (!baseline) return ["Initial draft has no published baseline"];
  const changes: string[] = [];
  if (current.name !== baseline.name) changes.push(`Name changed from “${baseline.name}”`);
  if (JSON.stringify(current.goal_policy) !== JSON.stringify(baseline.goal_policy)) {
    changes.push("Goal limits, deadline, or budget policy changed");
  }
  const currentNodes = new Map(current.nodes.map((node) => [node.node_id, node]));
  const baselineNodes = new Map(baseline.nodes.map((node) => [node.node_id, node]));
  for (const node of current.nodes) {
    const prior = baselineNodes.get(node.node_id);
    if (!prior) changes.push(`Node added: ${node.name}`);
    else if (JSON.stringify(node) !== JSON.stringify(prior)) {
      if (
        node.kind === "workflow" &&
        prior.kind === "workflow" &&
        node.pinned_definition_hash !== prior.pinned_definition_hash
      ) {
        changes.push(`Workflow definition changed: ${node.name}`);
      } else changes.push(`Node changed: ${node.name}`);
    }
  }
  for (const node of baseline.nodes) {
    if (!currentNodes.has(node.node_id)) changes.push(`Node removed: ${node.name}`);
  }
  const currentEdges = new Map(current.edges.map((edge) => [edge.edge_id, edge]));
  const baselineEdges = new Map(baseline.edges.map((edge) => [edge.edge_id, edge]));
  for (const edge of current.edges) {
    const prior = baselineEdges.get(edge.edge_id);
    if (!prior) changes.push(`Transition added: ${edge.transition_key}`);
    else if (JSON.stringify(edge) !== JSON.stringify(prior)) {
      changes.push(`Transition changed: ${edge.transition_key}`);
    }
  }
  for (const edge of baseline.edges) {
    if (!currentEdges.has(edge.edge_id)) changes.push(`Transition removed: ${edge.transition_key}`);
  }
  return changes;
}

export function PaletteItem({
  kind,
  label,
  icon,
  instanceCount = 0,
  disabled = false,
  onAdd,
}: {
  kind: string;
  label: string;
  icon: any;
  instanceCount?: number;
  disabled?: boolean;
  onAdd: (kind: string) => void;
}) {
  return (
    <div
      className="orch-palette-item"
      draggable={!disabled}
      role="group"
      aria-label={`${label} node`}
      onDragStart={(event) => {
        if (disabled) {
          event.preventDefault();
          return;
        }
        event.dataTransfer.setData("application/tandem-orchestration-node", kind);
        event.dataTransfer.effectAllowed = "copy";
      }}
      data-node-kind={kind}
    >
      <Icon name={icon} />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {instanceCount > 0 ? (
        <span
          className="orch-palette-count"
          aria-label={`${instanceCount} on canvas`}
          title={`${instanceCount} on canvas`}
        >
          {instanceCount}
        </span>
      ) : null}
      <button
        type="button"
        className="orch-palette-add nodrag"
        disabled={disabled}
        title={`Add ${label} node`}
        aria-label={`Add ${label} node`}
        onClick={() => onAdd(kind)}
      >
        <Icon name="plus" />
      </button>
    </div>
  );
}
