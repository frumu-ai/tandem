import { Icon } from "../../ui/Icon";
import type { GoalProjection, GoalSelection } from "./types";

function formatLabel(value: string) {
  return value.replaceAll("_", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function DetailValue({ value }: { value: unknown }) {
  if (value === null || value === undefined || value === "") {
    return <span className="goal-ops-empty-value">Not reported</span>;
  }
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return <span>{String(value)}</span>;
  }
  return <pre>{JSON.stringify(value, null, 2)}</pre>;
}

function recoveryDetails(projection: GoalProjection) {
  const recovery = projection.recovery;
  if (!recovery || typeof recovery !== "object" || Array.isArray(recovery)) return [];
  const details = recovery as Record<string, unknown>;
  return [
    {
      key: "resume_plan",
      value: details.resume_plan ?? (details.plan_id ? {
        plan_id: details.plan_id,
        safe_resume_points: details.safe_resume_points,
        operator_choices: details.operator_choices,
      } : null),
    },
    { key: "uncertain_effects", value: details.uncertain_effects },
    { key: "receipts", value: details.receipts ?? details.recovery_receipts ?? details.completed_effects },
    { key: "recovery_status", value: details.recovery_status ?? details.status ?? details.audit_summary },
  ].filter((entry) => entry.value !== undefined && entry.value !== null);
}

export function GoalOperationsInspector({
  projection,
  selection,
}: {
  projection: GoalProjection;
  selection: GoalSelection;
}) {
  const selectedNode = selection?.kind === "node"
    ? projection.graph.nodes.find((node) => node.node_id === selection.id) : undefined;
  const selectedEdge = selection?.kind === "edge"
    ? projection.graph.edges.find((edge) => edge.edge.edge_id === selection.id) : undefined;
  const selected = selectedNode || selectedEdge;
  const selectedNodeId = selection?.kind === "node" ? selection.id : "";
  const selectedEdgeId = selection?.kind === "edge" ? selection.id : "";
  const handoffs = projection.handoffs.filter((handoff) =>
    selectedNodeId
      ? handoff.source_node_id === selectedNodeId || handoff.target_node_id === selectedNodeId
      : selectedEdgeId ? handoff.edge_id === selectedEdgeId : true
  );
  const runIds = new Set(selectedNode
    ? selectedNode.runs.map((run) => run.run_id)
    : projection.workflow ? [projection.workflow.run_id] : []);
  const showsCurrentWorkflow = !selection || (!!projection.workflow && runIds.has(projection.workflow.run_id));
  const recoverySections = recoveryDetails(projection);
  const sections = [
    { key: "stage", value: showsCurrentWorkflow ? projection.workflow?.stage : null },
    { key: "checkpoint", value: showsCurrentWorkflow ? projection.workflow?.checkpoint : null },
    { key: "output", value: showsCurrentWorkflow ? projection.workflow?.outputs : null },
    { key: "retry", value: showsCurrentWorkflow ? projection.workflow?.retries : null },
    { key: "wait", value: projection.waits.filter((wait) => !runIds.size || runIds.has(wait.run_id)) },
    { key: "handoff", value: handoffs },
    { key: "artifacts", value: projection.artifacts.filter((artifact) => !runIds.size || runIds.has(artifact.source_run_id)) },
    ...recoverySections,
  ].filter((entry) => entry.value !== undefined && entry.value !== null && (!Array.isArray(entry.value) || entry.value.length));

  return (
    <aside className="goal-ops-inspector" aria-label="Goal execution inspector">
      <div className="goal-ops-panel-heading">
        <div>
          <span className="goal-ops-eyebrow">Inspector</span>
          <h2>{selectedNode?.name || selectedEdge?.edge.transition_key || "Current workflow"}</h2>
        </div>
        <Icon name="panel-right-open" size={16} />
      </div>
      {selected || projection.workflow || recoverySections.length ? (
        <div className="goal-ops-inspector-scroll">
          <dl className="goal-ops-facts">
            {selectedNode ? <><dt>Type</dt><dd>{typeof selectedNode.kind === "string" ? selectedNode.kind : selectedNode.kind.kind}</dd></> : null}
            {selectedNode ? <><dt>Status</dt><dd>{formatLabel(String(selectedNode.semantic_state || selectedNode.state))}</dd></> : null}
            {selection ? <><dt>ID</dt><dd className="goal-ops-mono">{selection.id}</dd></> : null}
          </dl>
          {sections.length ? sections.map((section) => (
            <section className="goal-ops-detail-section" key={section.key}>
              <h3>{formatLabel(section.key)}</h3>
              <DetailValue value={section.value} />
            </section>
          )) : (
            <p className="goal-ops-muted">No workflow or recovery details are reported yet.</p>
          )}
        </div>
      ) : (
        <p className="goal-ops-muted goal-ops-panel-pad">Select a node or transition to inspect its runtime details.</p>
      )}
    </aside>
  );
}
