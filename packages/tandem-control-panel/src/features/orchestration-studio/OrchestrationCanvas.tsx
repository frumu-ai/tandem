import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  ReactFlowProvider,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeChange,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { OrchestrationEdgeSpec, OrchestrationNodeSpec } from "@frumu/tandem-client";
import { useMemo, useRef } from "react";
import { Icon } from "../../ui/Icon";

export type OrchestrationSelection =
  { kind: "node"; id: string } | { kind: "edge"; id: string } | null;

type CanvasNodeData = {
  spec: OrchestrationNodeSpec;
  root: boolean;
  issueCount: number;
  stale: boolean;
  loop: boolean;
  readOnly?: boolean;
  onRemove?: (nodeId: string) => void;
};

function kindLabel(spec: OrchestrationNodeSpec) {
  if (spec.kind === "workflow") return "Workflow";
  if (spec.kind === "wait") return `${spec.wait.kind.replace("_", " ")} wait`;
  return `${spec.outcome} terminal`;
}

function nodeTone(spec: OrchestrationNodeSpec) {
  if (spec.kind === "terminal" && spec.outcome === "complete") return "complete";
  if (spec.kind === "terminal" && spec.outcome === "fail") return "failed";
  if (spec.kind === "wait") return "waiting";
  return "workflow";
}

function OrchestrationNode({ data, selected }: any) {
  const value = data as CanvasNodeData;
  return (
    <div
      className={`orch-node orch-node-${nodeTone(value.spec)} ${selected ? "selected" : ""}`}
      aria-label={`${value.spec.name}, ${kindLabel(value.spec)}${value.root ? ", root" : ""}`}
    >
      <Handle
        type="target"
        position={Position.Left}
        aria-label={`Connect into ${value.spec.name}`}
      />
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold">{value.spec.name}</div>
          <div className="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-[var(--color-text-subtle)]">
            <span>{kindLabel(value.spec)}</span>
            {value.root ? <span className="orch-node-tag">Root</span> : null}
            {value.stale ? <span className="orch-node-tag stale">Stale</span> : null}
            {value.loop ? <span className="orch-node-tag loop">Loop</span> : null}
          </div>
        </div>
        <div className="orch-node-actions">
          {value.issueCount ? (
            <span className="orch-issue-count" title={`${value.issueCount} validation issues`}>
              <Icon name="triangle-alert" size={13} />
              {value.issueCount}
            </span>
          ) : null}
          {!value.readOnly && value.onRemove ? (
            <button
              type="button"
              className="orch-node-remove nodrag nowheel"
              title={`Remove ${value.spec.name}`}
              aria-label={`Remove ${value.spec.name}`}
              onClick={(event) => {
                event.stopPropagation();
                value.onRemove?.(value.spec.node_id);
              }}
            >
              <Icon name="trash-2" size={14} />
            </button>
          ) : null}
        </div>
      </div>
      {value.spec.kind === "workflow" ? (
        <div className="mt-3 truncate font-mono text-xs text-[var(--color-text-subtle)]">
          {value.spec.automation_id}
        </div>
      ) : null}
      <Handle
        type="source"
        position={Position.Right}
        aria-label={`Connect from ${value.spec.name}`}
      />
    </div>
  );
}

const nodeTypes = { orchestration: OrchestrationNode };

export function toCanvasNodes(
  specs: OrchestrationNodeSpec[],
  rootNodeId: string,
  issueCounts: Map<string, number>,
  staleNodeIds: Set<string>,
  loopNodeIds: Set<string> = new Set()
): Node<CanvasNodeData>[] {
  return specs.map((spec, index) => ({
    id: spec.node_id,
    type: "orchestration",
    position: spec.position || { x: 72 + (index % 3) * 280, y: 72 + Math.floor(index / 3) * 180 },
    data: {
      spec,
      root: spec.node_id === rootNodeId,
      issueCount: issueCounts.get(spec.node_id) || 0,
      stale: staleNodeIds.has(spec.node_id),
      loop: loopNodeIds.has(spec.node_id),
    },
  }));
}

export function toCanvasEdges(
  specs: OrchestrationEdgeSpec[],
  loopEdgeIds: Set<string> = new Set()
): Edge[] {
  return specs.map((spec) => ({
    id: spec.edge_id,
    source: spec.from_node_id,
    target: spec.to_node_id,
    label: spec.transition_key,
    markerEnd: { type: MarkerType.ArrowClosed },
    className: `orch-edge ${loopEdgeIds.has(spec.edge_id) ? "loop" : ""}`,
    data: { spec },
  }));
}

export function OrchestrationCanvas({
  nodes,
  edges,
  rootNodeId,
  issueCounts,
  staleNodeIds,
  loopNodeIds,
  loopEdgeIds,
  selectedNodeIds,
  selectedEdgeIds,
  onSelectionChange,
  onSelectionSetChange,
  onNodesChange,
  onEdgesChange,
  onConnect,
  onReconnect,
  onDropNode,
  onDeleteNode,
  readOnly = false,
}: {
  nodes: OrchestrationNodeSpec[];
  edges: OrchestrationEdgeSpec[];
  rootNodeId: string;
  issueCounts: Map<string, number>;
  staleNodeIds: Set<string>;
  loopNodeIds: Set<string>;
  loopEdgeIds: Set<string>;
  selectedNodeIds: Set<string>;
  selectedEdgeIds: Set<string>;
  onSelectionChange: (selection: OrchestrationSelection) => void;
  onSelectionSetChange: (nodeIds: string[], edgeIds: string[]) => void;
  onNodesChange: (changes: NodeChange[]) => void;
  onEdgesChange: (changes: EdgeChange[]) => void;
  onConnect: (connection: Connection) => void;
  onReconnect: (edgeId: string, connection: Connection) => void;
  onDropNode: (kind: string, position: { x: number; y: number }) => void;
  onDeleteNode: (nodeId: string) => void;
  readOnly?: boolean;
}) {
  const flowRef = useRef<any>(null);
  const canvasNodes = useMemo(
    () =>
      toCanvasNodes(nodes, rootNodeId, issueCounts, staleNodeIds, loopNodeIds).map((node) => ({
        ...node,
        selected: selectedNodeIds.has(node.id),
        data: {
          ...node.data,
          readOnly,
          onRemove: onDeleteNode,
        },
      })),
    [
      issueCounts,
      loopNodeIds,
      nodes,
      onDeleteNode,
      readOnly,
      rootNodeId,
      selectedNodeIds,
      staleNodeIds,
    ]
  );
  const canvasEdges = useMemo(
    () =>
      toCanvasEdges(edges, loopEdgeIds).map((edge) => ({
        ...edge,
        selected: selectedEdgeIds.has(edge.id),
      })),
    [edges, loopEdgeIds, selectedEdgeIds]
  );

  return (
    <ReactFlowProvider>
      <div
        className="orch-canvas"
        onDragOver={(event) => {
          event.preventDefault();
          event.dataTransfer.dropEffect = "copy";
        }}
        onDrop={(event) => {
          event.preventDefault();
          const kind = event.dataTransfer.getData("application/tandem-orchestration-node");
          if (!kind) return;
          const position = flowRef.current?.screenToFlowPosition({
            x: event.clientX,
            y: event.clientY,
          });
          if (position) onDropNode(kind, position);
        }}
      >
        <ReactFlow
          onInit={(instance) => {
            flowRef.current = instance;
          }}
          nodes={canvasNodes}
          edges={canvasEdges}
          nodeTypes={nodeTypes}
          fitView
          minZoom={0.2}
          maxZoom={1.8}
          snapToGrid
          snapGrid={[16, 16]}
          nodesFocusable
          edgesFocusable
          nodesDraggable={!readOnly}
          nodesConnectable={!readOnly}
          multiSelectionKeyCode="Shift"
          deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
          onNodesChange={onNodesChange}
          onEdgesChange={onEdgesChange}
          onConnect={onConnect}
          onReconnect={(edge, connection) => onReconnect(edge.id, connection)}
          onSelectionChange={({ nodes: selectedNodes, edges: selectedEdges }) => {
            onSelectionSetChange(
              selectedNodes.map((node) => node.id),
              selectedEdges.map((edge) => edge.id)
            );
          }}
          onNodeClick={(_, node) => onSelectionChange({ kind: "node", id: node.id })}
          onNodeDoubleClick={(_, node) => {
            const spec = (node.data as CanvasNodeData).spec;
            if (spec.kind === "workflow") {
              window.location.hash = `#/studio?automation_id=${encodeURIComponent(spec.automation_id)}`;
            }
          }}
          onEdgeClick={(_, edge) => onSelectionChange({ kind: "edge", id: edge.id })}
          onPaneClick={() => onSelectionChange(null)}
          aria-label="Orchestration graph editor"
        >
          <Background gap={24} size={1} color="var(--color-border-subtle)" />
          <MiniMap
            pannable
            zoomable
            nodeColor={(node) => {
              const spec = (node.data as CanvasNodeData).spec;
              if (spec.kind === "terminal") return "#64748b";
              if (spec.kind === "wait") return "#d97706";
              return "#2563eb";
            }}
          />
          <Controls showInteractive={false} />
        </ReactFlow>
      </div>
    </ReactFlowProvider>
  );
}
