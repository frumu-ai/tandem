import { Icon, Spinner } from "../../ui";
import type { ChatWorkflowArtifact } from "./workflowArtifact";

export type WorkflowArtifactAction =
  | "validate"
  | "revise"
  | "open"
  | "duplicate"
  | "materialize"
  | "publish"
  | "enable";

type WorkflowArtifactCardProps = {
  artifact: ChatWorkflowArtifact;
  actionBusy?: WorkflowArtifactAction | "";
  toolStatus?: string;
  onAction: (action: WorkflowArtifactAction) => void;
};

function statusTone(value: string): string {
  const normalized = value.toLowerCase();
  if (["blocked", "failed", "invalid", "error"].some((part) => normalized.includes(part))) {
    return "danger";
  }
  if (["warning", "pending", "requested"].some((part) => normalized.includes(part))) {
    return "warning";
  }
  if (["ready", "valid", "complete", "success"].some((part) => normalized.includes(part))) {
    return "success";
  }
  return "neutral";
}

function ArtifactAction({
  action,
  label,
  icon,
  busy,
  disabled,
  onAction,
}: {
  action: WorkflowArtifactAction;
  label: string;
  icon: "badge-check" | "pencil" | "external-link" | "copy" | "file-plus" | "rocket" | "play";
  busy: boolean;
  disabled?: boolean;
  onAction: (action: WorkflowArtifactAction) => void;
}) {
  return (
    <button
      type="button"
      className="chat-workflow-action"
      disabled={disabled || busy}
      onClick={() => onAction(action)}
    >
      {busy ? <Spinner label={`${label} in progress`} /> : <Icon name={icon} />}
      <span>{label}</span>
    </button>
  );
}

export function WorkflowArtifactCard({
  artifact,
  actionBusy = "",
  toolStatus = "",
  onAction,
}: WorkflowArtifactCardProps) {
  const operationActive = artifact.operationStatus === "running";
  const activity = operationActive
    ? `${artifact.operationKind || "Workflow tool"} running`
    : artifact.operationError
      ? artifact.operationError
      : toolStatus;
  const details = [
    ["Outputs", artifact.outputs],
    ["Assumptions", artifact.assumptions],
    ["Connections", artifact.connections],
    ["Approvals", artifact.approvals],
    ["Constraints", artifact.constraints],
  ] as const;
  const hasDetails = details.some(([, values]) => values.length);
  const canMaterialize = artifact.lifecycle === "draft";
  const canPublish = artifact.lifecycle === "materialized";
  const canEnable = artifact.lifecycle !== "draft" && Boolean(artifact.automationUrl);

  return (
    <article
      className="chat-workflow-artifact"
      aria-label={`Workflow artifact: ${artifact.title}`}
      data-testid="chat-workflow-artifact"
    >
      <header className="chat-workflow-header">
        <div className="chat-workflow-heading">
          <span className="chat-workflow-icon" aria-hidden="true">
            <Icon name="workflow" />
          </span>
          <div className="min-w-0">
            <div className="chat-workflow-eyebrow">Workflow artifact</div>
            <h3 className="chat-workflow-title">{artifact.title}</h3>
          </div>
        </div>
        <div className="chat-workflow-badges" aria-label="Artifact status">
          <span className={`chat-workflow-badge ${artifact.lifecycle}`}>{artifact.lifecycle}</span>
          <span className="chat-workflow-badge">rev {artifact.revision}</span>
          <span className={`chat-workflow-badge ${statusTone(artifact.validationStatus)}`}>
            {artifact.validationStatus}
          </span>
        </div>
      </header>

      {artifact.description ? <p className="chat-workflow-description">{artifact.description}</p> : null}

      {activity ? (
        <div
          className={`chat-workflow-activity ${artifact.operationError ? "failed" : ""}`}
          role="status"
          aria-live="polite"
        >
          {operationActive ? <Spinner label={activity} /> : <Icon name="activity" />}
          <span>{activity}</span>
        </div>
      ) : null}

      <div className="chat-workflow-flow" aria-label="Workflow structure">
        <div className="chat-workflow-trigger">
          <Icon name="radio" />
          <span>{artifact.trigger}</span>
        </div>
        {artifact.stages.length ? (
          artifact.stages.map((stage, stageIndex) => (
            <section
              key={stage.id}
              className="chat-workflow-stage"
              aria-label={stage.parallel ? `Parallel stage ${stageIndex + 1}` : `Stage ${stageIndex + 1}`}
            >
              <div className="chat-workflow-stage-label">
                <span>{stage.parallel ? "Runs together" : `Step ${stageIndex + 1}`}</span>
                {stage.parallel ? <strong>{stage.nodes.length} parallel</strong> : null}
              </div>
              <div className={`chat-workflow-nodes ${stage.parallel ? "parallel" : ""}`} role="list">
                {stage.nodes.map((node) => (
                  <div key={node.id} className="chat-workflow-node" role="listitem">
                    <div className="chat-workflow-node-topline">
                      <span className="chat-workflow-node-kind">{node.kind}</span>
                      {node.agentRole ? <span>{node.agentRole}</span> : null}
                    </div>
                    <div className="chat-workflow-node-objective">{node.objective}</div>
                    {node.dependencies.length ? (
                      <div className="chat-workflow-node-meta">
                        after {node.dependencies.join(", ")}
                      </div>
                    ) : null}
                    {node.output ? (
                      <div className="chat-workflow-node-meta">output: {node.output}</div>
                    ) : null}
                  </div>
                ))}
              </div>
            </section>
          ))
        ) : (
          <div className="chat-workflow-empty">Workflow structure is being prepared.</div>
        )}
      </div>

      {artifact.blockers.length || artifact.warnings.length ? (
        <div className="chat-workflow-notices">
          {artifact.blockers.map((blocker) => (
            <div key={`blocker-${blocker}`} className="chat-workflow-notice blocker">
              <Icon name="shield-alert" />
              <span>{blocker}</span>
            </div>
          ))}
          {artifact.warnings.map((warning) => (
            <div key={`warning-${warning}`} className="chat-workflow-notice warning">
              <Icon name="triangle-alert" />
              <span>{warning}</span>
            </div>
          ))}
        </div>
      ) : null}

      {hasDetails ? (
        <details className="chat-workflow-details">
          <summary>
            <Icon name="list-tree" />
            Artifact details
          </summary>
          <div className="chat-workflow-detail-grid">
            {details.map(([label, values]) =>
              values.length ? (
                <section key={label}>
                  <h4>{label}</h4>
                  <ul>
                    {values.map((value) => (
                      <li key={value}>{value}</li>
                    ))}
                  </ul>
                </section>
              ) : null
            )}
          </div>
        </details>
      ) : null}

      <footer className="chat-workflow-actions" aria-label="Workflow actions">
        <ArtifactAction
          action="validate"
          label="Validate"
          icon="badge-check"
          busy={actionBusy === "validate"}
          disabled={operationActive}
          onAction={onAction}
        />
        <ArtifactAction
          action="revise"
          label="Revise"
          icon="pencil"
          busy={actionBusy === "revise"}
          disabled={operationActive}
          onAction={onAction}
        />
        <ArtifactAction
          action="open"
          label="Open canvas"
          icon="external-link"
          busy={actionBusy === "open"}
          onAction={onAction}
        />
        <ArtifactAction
          action="duplicate"
          label="Duplicate"
          icon="copy"
          busy={actionBusy === "duplicate"}
          disabled={operationActive}
          onAction={onAction}
        />
        {canMaterialize ? (
          <ArtifactAction
            action="materialize"
            label="Create draft"
            icon="file-plus"
            busy={actionBusy === "materialize"}
            disabled={operationActive}
            onAction={onAction}
          />
        ) : null}
        {canPublish ? (
          <ArtifactAction
            action="publish"
            label="Publish"
            icon="rocket"
            busy={actionBusy === "publish"}
            disabled={operationActive}
            onAction={onAction}
          />
        ) : null}
        {canEnable ? (
          <ArtifactAction
            action="enable"
            label="Enable"
            icon="play"
            busy={actionBusy === "enable"}
            disabled={operationActive}
            onAction={onAction}
          />
        ) : null}
      </footer>
    </article>
  );
}
