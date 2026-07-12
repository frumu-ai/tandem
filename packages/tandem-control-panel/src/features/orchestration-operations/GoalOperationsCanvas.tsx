import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  ReactFlowProvider,
  type Edge,
  type Node,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useMemo } from "react";
import type { GoalProjection, GoalProjectionNode, GoalSelection } from "./types";

function nodeKindLabel(node: GoalProjectionNode): string {
  return typeof node.kind === "string" ? node.kind : node.kind.kind;
}

function isPosition(value: unknown): value is { x: number; y: number } {
  return !!value && typeof value === "object" &&
    typeof (value as any).x === "number" && typeof (value as any).y === "number";
}

function stateLabel(node: GoalProjectionNode): string {
  return String(node.semantic_state || node.state || "not started").replaceAll("_", " ");
}

function OperationsNode({ data, selected }: { data: GoalProjectionNode; selected: boolean }) {
  const status = stateLabel(data);
  const kind = nodeKindLabel(data);
  return (
    <div
      className={`goal-ops-node state-${status.replaceAll(" ", "-")} ${selected ? "selected" : ""}`}
      aria-label={`${data.name}, ${kind}, status ${status}`}
    >
      <Handle type="target" position={Position.Left} isConnectable={false} />
      <div className="goal-ops-node-title">{data.name}</div>
      <div className="goal-ops-node-meta">
        <span>{kind}</span>
        <span className="goal-ops-status-text">{status}</span>
      </div>
      <Handle type="source" position={Position.Right} isConnectable={false} />
    </div>
  );
}

const nodeTypes = { operation: OperationsNode };

function GoalOperationsCanvasInner({
  goalId,
  projection,
  selection,
  onSelect,
}: {
  goalId: string;
  projection: GoalProjection;
  selection: GoalSelection;
  onSelect: (selection: GoalSelection) => void;
}) {
  const nodes = useMemo<Node<GoalProjectionNode>[]>(
    () =>
      projection.graph.nodes.map((node, index) => {
        const spec = projection.orchestration?.nodes.find((item) => item.node_id === node.node_id);
        return {
          id: node.node_id,
          type: "operation",
          position: isPosition(node.position) ? node.position : spec?.position || {
            x: 60 + (index % 3) * 260,
            y: 60 + Math.floor(index / 3) * 150,
          },
          data: node,
          selected: selection?.kind === "node" && selection.id === node.node_id,
          draggable: false,
          connectable: false,
        };
      }),
    [projection.graph.nodes, projection.orchestration, selection]
  );
  const edges = useMemo<Edge[]>(
    () =>
      projection.graph.edges.map(({ edge, state }) => ({
        id: edge.edge_id,
        source: edge.from_node_id,
        target: edge.to_node_id,
        label: edge.transition_key,
        markerEnd: { type: MarkerType.ArrowClosed },
        className: `goal-ops-edge state-${state}`,
        selected: selection?.kind === "edge" && selection.id === edge.edge_id,
      })),
    [projection.graph.edges, selection]
  );

  return (
    <div className="goal-ops-canvas" data-testid="goal-operations-canvas" data-goal-id={goalId}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.18 }}
        onNodeClick={(_, node) => onSelect({ kind: "node", id: node.id })}
        onEdgeClick={(_, edge) => onSelect({ kind: "edge", id: edge.id })}
        onPaneClick={() => onSelect(null)}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        panOnDrag
        zoomOnScroll
        minZoom={0.25}
        maxZoom={1.7}
        deleteKeyCode={null}
        aria-label="Read-only goal orchestration canvas"
      >
        <Background gap={24} size={1} color="var(--color-border-subtle)" />
        <MiniMap pannable zoomable aria-label="Goal graph minimap" />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  );
}

export function GoalOperationsCanvas(props: Parameters<typeof GoalOperationsCanvasInner>[0]) {
  return (
    <ReactFlowProvider>
      <GoalOperationsCanvasInner {...props} />
    </ReactFlowProvider>
  );
}
