import type {
  OrchestrationEdgeSpec,
  OrchestrationGoalPolicy,
  OrchestrationNodeSpec,
  OrchestrationValueBinding,
  OrchestrationWaitSpec,
} from "@frumu/tandem-client";
import { Icon } from "../../ui/Icon";
import type { OrchestrationSelection } from "./OrchestrationCanvas";

const fieldClass = "tcp-input w-full";

function numberOrUndefined(value: string) {
  const parsed = Number(value);
  return value.trim() && Number.isFinite(parsed) ? parsed : undefined;
}

function localDateTimeValue(timestamp?: number) {
  if (!timestamp) return "";
  const date = new Date(timestamp);
  return new Date(timestamp - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
}

function Field({ label, children, hint }: { label: string; children: any; hint?: string }) {
  return (
    <label className="grid gap-1.5 text-xs font-medium">
      <span>{label}</span>
      {children}
      {hint ? <span className="font-normal text-[var(--color-text-subtle)]">{hint}</span> : null}
    </label>
  );
}

function BindingFields({
  binding,
  nodes,
  onChange,
  label,
  literalType = "string",
}: {
  binding: OrchestrationValueBinding;
  nodes: OrchestrationNodeSpec[];
  onChange: (binding: OrchestrationValueBinding) => void;
  label: string;
  literalType?: "string" | "number";
}) {
  return (
    <div className="grid gap-3 border-l-2 border-[var(--color-border-subtle)] pl-3">
      <Field label={`${label} source`}>
        <select
          className={fieldClass}
          value={binding.source}
          onChange={(event) =>
            onChange(
              event.currentTarget.value === "node_output"
                ? { source: "node_output", node_id: nodes[0]?.node_id || "" }
                : { source: "literal", value: "" }
            )
          }
        >
          <option value="literal">Literal value</option>
          <option value="node_output" disabled={!nodes.length}>
            Upstream node output
          </option>
        </select>
      </Field>
      {binding.source === "literal" ? (
        <Field label={`${label} value`}>
          <input
            className={fieldClass}
            type={literalType === "number" ? "number" : "text"}
            value={String(binding.value ?? "")}
            onInput={(event) =>
              onChange({
                source: "literal",
                value:
                  literalType === "number"
                    ? Number(event.currentTarget.value)
                    : event.currentTarget.value,
              })
            }
          />
        </Field>
      ) : (
        <>
          <Field label="Source node">
            <select
              className={fieldClass}
              value={binding.node_id}
              onChange={(event) => onChange({ ...binding, node_id: event.currentTarget.value })}
            >
              {nodes.map((node) => (
                <option key={node.node_id} value={node.node_id}>
                  {node.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label="JSON pointer" hint="Optional path such as /result/id">
            <input
              className={fieldClass}
              value={binding.json_pointer || ""}
              onInput={(event) =>
                onChange({
                  ...binding,
                  json_pointer: event.currentTarget.value || undefined,
                })
              }
            />
          </Field>
        </>
      )}
    </div>
  );
}

function WaitFields({
  wait,
  nodes,
  onChange,
}: {
  wait: OrchestrationWaitSpec;
  nodes: OrchestrationNodeSpec[];
  onChange: (wait: OrchestrationWaitSpec) => void;
}) {
  const timeout = "timeout" in wait ? wait.timeout : undefined;
  const updateTimeout = (patch: Record<string, unknown>) => {
    const base = timeout || { expires_after_ms: 3_600_000, on_timeout: "cancel" };
    onChange({ ...wait, timeout: { ...base, ...patch } } as OrchestrationWaitSpec);
  };
  return (
    <div className="grid gap-3">
      <Field label="Wait kind">
        <select
          className={fieldClass}
          value={wait.kind}
          onChange={(event) => {
            const kind = event.currentTarget.value;
            if (kind === "timer") onChange({ kind: "timer", delay_ms: 3_600_000 });
            if (kind === "approval")
              onChange({ kind: "approval", decisions: ["approve", "reject"] });
            if (kind === "webhook")
              onChange({
                kind: "webhook",
                trigger_id: "",
                correlation: {
                  field: "provider_event_id",
                  value: { source: "literal", value: "" },
                },
                timeout: { expires_after_ms: 86_400_000, on_timeout: "cancel" },
              });
            if (kind === "external_condition")
              onChange({
                kind: "external_condition",
                condition_key: { source: "literal", value: "" },
                timeout: { expires_after_ms: 86_400_000, on_timeout: "cancel" },
              });
          }}
        >
          <option value="timer">Timer</option>
          <option value="approval">Approval</option>
          <option value="webhook">Correlated webhook</option>
          <option value="external_condition">External condition</option>
        </select>
      </Field>
      {wait.kind === "timer" ? (
        <>
          <Field label="Timer source">
            <select
              className={fieldClass}
              value={wait.wake_at ? "wake_at" : "delay"}
              onChange={(event) =>
                onChange(
                  event.currentTarget.value === "wake_at"
                    ? {
                        kind: "timer",
                        wake_at: { source: "literal", value: Date.now() },
                        timeout: wait.timeout,
                      }
                    : { kind: "timer", delay_ms: 3_600_000, timeout: wait.timeout }
                )
              }
            >
              <option value="delay">Relative delay</option>
              <option value="wake_at">Resolved wake timestamp</option>
            </select>
          </Field>
          {wait.wake_at ? (
            <BindingFields
              label="Wake timestamp"
              binding={wait.wake_at}
              nodes={nodes}
              literalType="number"
              onChange={(wake_at) => onChange({ ...wait, wake_at, delay_ms: undefined })}
            />
          ) : (
            <Field label="Delay (milliseconds)">
              <input
                className={fieldClass}
                type="number"
                min="1"
                value={wait.delay_ms || ""}
                onInput={(event) =>
                  onChange({ ...wait, delay_ms: numberOrUndefined(event.currentTarget.value) })
                }
              />
            </Field>
          )}
        </>
      ) : null}
      {wait.kind === "approval" ? (
        <>
          <Field label="Allowed decisions" hint="Comma-separated decision keys">
            <input
              className={fieldClass}
              value={wait.decisions.join(", ")}
              onInput={(event) =>
                onChange({
                  ...wait,
                  decisions: event.currentTarget.value
                    .split(",")
                    .map((value) => value.trim())
                    .filter(Boolean),
                })
              }
            />
          </Field>
          <Field label="Expires after (milliseconds)">
            <input
              className={fieldClass}
              type="number"
              min="1"
              value={wait.expires_after_ms || ""}
              onInput={(event) =>
                onChange({
                  ...wait,
                  expires_after_ms: numberOrUndefined(event.currentTarget.value),
                })
              }
            />
          </Field>
        </>
      ) : null}
      {wait.kind === "webhook" ? (
        <>
          <Field label="Webhook trigger ID">
            <input
              className={fieldClass}
              value={wait.trigger_id}
              onInput={(event) => onChange({ ...wait, trigger_id: event.currentTarget.value })}
            />
          </Field>
          <Field label="Correlation field">
            <select
              className={fieldClass}
              value={wait.correlation.field}
              onChange={(event) =>
                onChange({
                  ...wait,
                  correlation: { ...wait.correlation, field: event.currentTarget.value as any },
                })
              }
            >
              <option value="provider_event_id">Provider event ID</option>
              <option value="idempotency_key">Idempotency key</option>
              <option value="body_digest">Body digest</option>
            </select>
          </Field>
          <BindingFields
            label="Correlation"
            binding={wait.correlation.value}
            nodes={nodes}
            onChange={(value) => onChange({ ...wait, correlation: { ...wait.correlation, value } })}
          />
          <Field label="Provider">
            <input
              className={fieldClass}
              value={wait.provider || ""}
              onInput={(event) =>
                onChange({ ...wait, provider: event.currentTarget.value || undefined })
              }
            />
          </Field>
          <Field label="Provider event kind">
            <input
              className={fieldClass}
              value={wait.provider_event_kind || ""}
              onInput={(event) =>
                onChange({
                  ...wait,
                  provider_event_kind: event.currentTarget.value || undefined,
                })
              }
            />
          </Field>
        </>
      ) : null}
      {wait.kind === "external_condition" ? (
        <>
          <BindingFields
            label="Condition key"
            binding={wait.condition_key}
            nodes={nodes}
            onChange={(condition_key) => onChange({ ...wait, condition_key })}
          />
          <Field label="Resolution payload schema (JSON)">
            <textarea
              key={JSON.stringify(wait.payload_schema || {})}
              className={`${fieldClass} min-h-24 font-mono text-xs`}
              defaultValue={JSON.stringify(wait.payload_schema || {}, null, 2)}
              onBlur={(event) => {
                try {
                  onChange({
                    ...wait,
                    payload_schema: JSON.parse(event.currentTarget.value || "{}"),
                  });
                } catch {
                  event.currentTarget.setCustomValidity("Enter a valid JSON schema");
                  event.currentTarget.reportValidity();
                }
              }}
              onInput={(event) => event.currentTarget.setCustomValidity("")}
            />
          </Field>
        </>
      ) : null}
      {wait.kind === "timer" ? (
        <label className="flex items-center gap-2 text-xs font-medium">
          <input
            type="checkbox"
            checked={!!wait.timeout}
            onChange={(event) =>
              onChange({
                ...wait,
                timeout: event.currentTarget.checked
                  ? { expires_after_ms: 86_400_000, on_timeout: "cancel" }
                  : undefined,
              })
            }
          />
          Add timeout policy
        </label>
      ) : null}
      {wait.kind !== "timer" || timeout ? (
        <>
          <Field label="Timeout (milliseconds)">
            <input
              className={fieldClass}
              type="number"
              min="1"
              value={timeout?.expires_after_ms || ""}
              onInput={(event) =>
                updateTimeout({ expires_after_ms: numberOrUndefined(event.currentTarget.value) })
              }
            />
          </Field>
          <Field label="On timeout">
            <select
              className={fieldClass}
              value={timeout?.on_timeout || "cancel"}
              onChange={(event) => updateTimeout({ on_timeout: event.currentTarget.value })}
            >
              <option value="cancel">Cancel</option>
              <option value="escalate">Escalate</option>
              <option value="remind">Remind</option>
              <option value="resume">Resume</option>
            </select>
          </Field>
          {timeout?.on_timeout === "escalate" ? (
            <Field label="Escalation principal or queue">
              <input
                className={fieldClass}
                value={timeout.escalate_to || ""}
                onInput={(event) => updateTimeout({ escalate_to: event.currentTarget.value })}
              />
            </Field>
          ) : null}
          {timeout?.on_timeout === "remind" ? (
            <Field label="Reminder interval (milliseconds)">
              <input
                className={fieldClass}
                type="number"
                min="1"
                value={timeout.remind_every_ms || ""}
                onInput={(event) =>
                  updateTimeout({ remind_every_ms: numberOrUndefined(event.currentTarget.value) })
                }
              />
            </Field>
          ) : null}
        </>
      ) : null}
    </div>
  );
}

export function OrchestrationInspector({
  selection,
  nodes,
  edges,
  rootNodeId,
  policy,
  onNodeChange,
  onEdgeChange,
  onPolicyChange,
  onSetRoot,
  onDelete,
  readOnly = false,
}: {
  selection: OrchestrationSelection;
  nodes: OrchestrationNodeSpec[];
  edges: OrchestrationEdgeSpec[];
  rootNodeId: string;
  policy: OrchestrationGoalPolicy;
  onNodeChange: (node: OrchestrationNodeSpec) => void;
  onEdgeChange: (edge: OrchestrationEdgeSpec) => void;
  onPolicyChange: (policy: OrchestrationGoalPolicy) => void;
  onSetRoot: (nodeId: string) => void;
  onDelete: () => void;
  readOnly?: boolean;
}) {
  const node =
    selection?.kind === "node" ? nodes.find((item) => item.node_id === selection.id) : null;
  const edge =
    selection?.kind === "edge" ? edges.find((item) => item.edge_id === selection.id) : null;
  const upstreamNodes = (() => {
    if (!node) return nodes;
    const byId = new Map(nodes.map((candidate) => [candidate.node_id, candidate]));
    const upstreamIds = new Set<string>();
    const queue = edges
      .filter((candidate) => candidate.to_node_id === node.node_id)
      .map((candidate) => candidate.from_node_id);
    for (let index = 0; index < queue.length; index += 1) {
      const nodeId = queue[index];
      if (upstreamIds.has(nodeId) || nodeId === node.node_id) continue;
      upstreamIds.add(nodeId);
      edges
        .filter((candidate) => candidate.to_node_id === nodeId)
        .forEach((candidate) => queue.push(candidate.from_node_id));
    }
    return [...upstreamIds]
      .map((nodeId) => byId.get(nodeId))
      .filter(Boolean) as OrchestrationNodeSpec[];
  })();

  return (
    <aside className="orch-inspector" aria-label="Orchestration inspector">
      <div className="flex items-center justify-between gap-3 border-b border-[var(--color-border-subtle)] px-4 py-3">
        <div>
          <div className="text-sm font-semibold">Inspector</div>
          <div className="text-xs text-[var(--color-text-subtle)]">
            {node ? "Node settings" : edge ? "Transition settings" : "Goal policy"}
          </div>
        </div>
        {selection && !readOnly ? (
          <button
            className="tcp-icon-btn"
            title="Delete selected item"
            aria-label="Delete selected item"
            onClick={onDelete}
          >
            <Icon name="trash-2" />
          </button>
        ) : null}
      </div>
      <fieldset disabled={readOnly} className="grid gap-4 overflow-y-auto p-4">
        {node ? (
          <>
            <Field label="Name">
              <input
                className={fieldClass}
                value={node.name}
                onInput={(event) => onNodeChange({ ...node, name: event.currentTarget.value })}
              />
            </Field>
            <button
              className={node.node_id === rootNodeId ? "tcp-btn-primary" : "tcp-btn"}
              disabled={node.kind !== "workflow" || node.node_id === rootNodeId}
              onClick={() => onSetRoot(node.node_id)}
            >
              <Icon name="target" />
              {node.node_id === rootNodeId ? "Root workflow" : "Set as root"}
            </button>
            {node.kind === "workflow" ? (
              <>
                <Field label="Automation ID">
                  <input className={fieldClass} value={node.automation_id} readOnly />
                </Field>
                <Field
                  label="Allowed transition keys"
                  hint="Comma-separated keys exposed to agents"
                >
                  <input
                    className={fieldClass}
                    value={(node.allowed_transition_keys || []).join(", ")}
                    readOnly
                  />
                </Field>
                <Field label="Accepted artifact types" hint="Comma-separated contract names">
                  <input
                    className={fieldClass}
                    value={(node.accepts_artifact_types || []).join(", ")}
                    onInput={(event) =>
                      onNodeChange({
                        ...node,
                        accepts_artifact_types: event.currentTarget.value
                          .split(",")
                          .map((value) => value.trim())
                          .filter(Boolean),
                      })
                    }
                  />
                </Field>
                <Field label="Emitted artifact types" hint="Comma-separated contract names">
                  <input
                    className={fieldClass}
                    value={(node.emits_artifact_types || []).join(", ")}
                    onInput={(event) =>
                      onNodeChange({
                        ...node,
                        emits_artifact_types: event.currentTarget.value
                          .split(",")
                          .map((value) => value.trim())
                          .filter(Boolean),
                      })
                    }
                  />
                </Field>
              </>
            ) : null}
            {node.kind === "wait" ? (
              <WaitFields
                wait={node.wait}
                nodes={upstreamNodes}
                onChange={(wait) => onNodeChange({ ...node, wait })}
              />
            ) : null}
            {node.kind === "terminal" ? (
              <>
                <Field label="Outcome">
                  <select
                    className={fieldClass}
                    value={node.outcome}
                    onChange={(event) =>
                      onNodeChange({ ...node, outcome: event.currentTarget.value as any })
                    }
                  >
                    <option value="complete">Complete</option>
                    <option value="pause">Pause</option>
                    <option value="fail">Fail</option>
                  </select>
                </Field>
                <Field label="Final artifact type">
                  <input
                    className={fieldClass}
                    value={node.final_artifact_type || ""}
                    onInput={(event) =>
                      onNodeChange({
                        ...node,
                        final_artifact_type: event.currentTarget.value || undefined,
                      })
                    }
                  />
                </Field>
              </>
            ) : null}
          </>
        ) : edge ? (
          <>
            <Field label="Transition key">
              <input
                className={fieldClass}
                value={edge.transition_key}
                onInput={(event) =>
                  onEdgeChange({ ...edge, transition_key: event.currentTarget.value })
                }
              />
            </Field>
            <Field label="Artifact type">
              <input
                className={fieldClass}
                value={edge.artifact_contract?.artifact_type || ""}
                onInput={(event) =>
                  onEdgeChange({
                    ...edge,
                    artifact_contract: event.currentTarget.value
                      ? {
                          artifact_type: event.currentTarget.value,
                          required: edge.artifact_contract?.required ?? true,
                          schema: edge.artifact_contract?.schema,
                        }
                      : undefined,
                  })
                }
              />
            </Field>
            <label className="flex items-center gap-2 text-xs font-medium">
              <input
                type="checkbox"
                checked={edge.artifact_contract?.required ?? false}
                disabled={!edge.artifact_contract}
                onChange={(event) =>
                  onEdgeChange({
                    ...edge,
                    artifact_contract: edge.artifact_contract
                      ? { ...edge.artifact_contract, required: event.currentTarget.checked }
                      : undefined,
                  })
                }
              />
              Artifact is required
            </label>
            <Field label="Artifact schema (JSON)">
              <textarea
                key={`${edge.edge_id}:${JSON.stringify(edge.artifact_contract?.schema || {})}`}
                className={`${fieldClass} min-h-24 font-mono text-xs`}
                defaultValue={JSON.stringify(edge.artifact_contract?.schema || {}, null, 2)}
                disabled={!edge.artifact_contract}
                onBlur={(event) => {
                  if (!edge.artifact_contract) return;
                  try {
                    onEdgeChange({
                      ...edge,
                      artifact_contract: {
                        ...edge.artifact_contract,
                        schema: JSON.parse(event.currentTarget.value || "{}"),
                      },
                    });
                  } catch {
                    event.currentTarget.setCustomValidity("Enter valid JSON schema");
                    event.currentTarget.reportValidity();
                  }
                }}
                onInput={(event) => event.currentTarget.setCustomValidity("")}
              />
            </Field>
            <label className="flex items-center gap-2 text-xs font-medium">
              <input
                type="checkbox"
                checked={edge.approval?.required ?? false}
                onChange={(event) =>
                  onEdgeChange({
                    ...edge,
                    approval: event.currentTarget.checked
                      ? { required: true, expires_after_ms: 86_400_000 }
                      : undefined,
                  })
                }
              />
              Require approval before transition
            </label>
            {edge.approval?.required ? (
              <>
                <Field label="Approval expiry (milliseconds)">
                  <input
                    className={fieldClass}
                    type="number"
                    min="1"
                    value={edge.approval.expires_after_ms || ""}
                    onInput={(event) =>
                      onEdgeChange({
                        ...edge,
                        approval: {
                          ...edge.approval!,
                          expires_after_ms: numberOrUndefined(event.currentTarget.value),
                        },
                      })
                    }
                  />
                </Field>
                <Field label="Approver scope">
                  <input
                    className={fieldClass}
                    value={edge.approval.approver_scope || ""}
                    onInput={(event) =>
                      onEdgeChange({
                        ...edge,
                        approval: { ...edge.approval!, approver_scope: event.currentTarget.value },
                      })
                    }
                  />
                </Field>
              </>
            ) : null}
          </>
        ) : (
          <>
            <Field label="Maximum workflow hops" hint="Finite hop limits are required for loops">
              <input
                className={fieldClass}
                type="number"
                min="1"
                value={policy.max_hops}
                onInput={(event) =>
                  onPolicyChange({
                    ...policy,
                    max_hops: Math.max(1, Number(event.currentTarget.value)),
                  })
                }
              />
            </Field>
            <Field label="Token budget">
              <input
                className={fieldClass}
                type="number"
                min="1"
                value={policy.max_total_tokens || ""}
                onInput={(event) =>
                  onPolicyChange({
                    ...policy,
                    max_total_tokens: numberOrUndefined(event.currentTarget.value),
                  })
                }
              />
            </Field>
            <Field label="Deadline">
              <input
                className={fieldClass}
                type="datetime-local"
                value={localDateTimeValue(policy.deadline_at_ms)}
                onInput={(event) =>
                  onPolicyChange({
                    ...policy,
                    deadline_at_ms: event.currentTarget.value
                      ? new Date(event.currentTarget.value).getTime()
                      : undefined,
                  })
                }
              />
            </Field>
            <Field label="Cost budget (USD)">
              <input
                className={fieldClass}
                type="number"
                min="0"
                step="0.01"
                value={policy.max_total_cost_usd || ""}
                onInput={(event) =>
                  onPolicyChange({
                    ...policy,
                    max_total_cost_usd: numberOrUndefined(event.currentTarget.value),
                  })
                }
              />
            </Field>
            <Field label="When a limit is reached">
              <select
                className={fieldClass}
                value={policy.on_limit}
                onChange={(event) =>
                  onPolicyChange({ ...policy, on_limit: event.currentTarget.value as any })
                }
              >
                <option value="pause_for_review">Pause for review</option>
                <option value="fail">Fail the goal</option>
              </select>
            </Field>
          </>
        )}
      </fieldset>
    </aside>
  );
}
