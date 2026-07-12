import { useMemo, useState } from "react";
import { ConfirmDialog } from "../../components/ControlPanelDialogs";
import { Icon } from "../../ui/Icon";
import type {
  GoalActionPayloadField,
  GoalActionPayloadOption,
  GoalOperationMode,
  GoalProjectionAction,
} from "./types";

export type GoalActionInput = {
  reason?: string;
  decision?: string;
  payload?: Record<string, unknown>;
};

function optionsFor(action: GoalProjectionAction) {
  return (action.decision_options || []).map((option) =>
    typeof option === "string" ? { value: option, label: option } : option
  );
}

function payloadOptions(options: GoalActionPayloadOption[] | null | undefined) {
  return (options || []).map((option) =>
    typeof option === "string" ? { value: option, label: option } : option
  );
}

function initialPayload(action: GoalProjectionAction) {
  return Object.fromEntries((action.payload_fields || []).map((field) => [
    field.name,
    field.required ? payloadOptions(field.options)[0]?.value || "" : "",
  ]));
}

function payloadError(fields: GoalActionPayloadField[], payload: Record<string, string>) {
  for (const field of fields) {
    const value = String(payload[field.name] || "").trim();
    if (field.required && !value) return `${field.label} is required.`;
    if (field.format === "json" && value) {
      try {
        JSON.parse(value);
      } catch {
        return `${field.label} must contain valid JSON.`;
      }
    }
  }
  return "";
}

export function GoalActionControls({
  actions,
  mode,
  pendingActionId,
  onPerform,
}: {
  actions: GoalProjectionAction[];
  mode: GoalOperationMode;
  pendingActionId: string;
  onPerform: (action: GoalProjectionAction, input: GoalActionInput) => Promise<void>;
}) {
  const [selected, setSelected] = useState<GoalProjectionAction | null>(null);
  const [reason, setReason] = useState("");
  const [decision, setDecision] = useState("");
  const [payload, setPayload] = useState<Record<string, string>>({});
  const decisionOptions = useMemo(() => selected ? optionsFor(selected) : [], [selected]);
  const payloadFields = selected?.payload_fields || [];
  const invalidPayload = payloadError(payloadFields, payload);

  const begin = (action: GoalProjectionAction) => {
    setReason("");
    setDecision(optionsFor(action)[0]?.value || "");
    setPayload(initialPayload(action));
    setSelected(action);
  };
  const submit = async () => {
    if (!selected) return;
    if (invalidPayload) return;
    const structuredPayload = Object.fromEntries(payloadFields.flatMap((field) => {
      const value = String(payload[field.name] || "").trim();
      if (!value) return [];
      return [[field.name, field.format === "json" ? JSON.parse(value) : value]];
    }));
    await onPerform(selected, {
      ...(reason.trim() ? { reason: reason.trim() } : {}),
      ...(decision ? { decision } : {}),
      ...(Object.keys(structuredPayload).length ? { payload: structuredPayload } : {}),
    });
    setSelected(null);
  };

  const setPayloadField = (field: GoalActionPayloadField, value: string) => {
    setPayload((current) => ({ ...current, [field.name]: value }));
  };

  return (
    <section className="goal-ops-actions" aria-label="Governed goal actions">
      <div className="goal-ops-actions-heading">
        <div>
          <span className="goal-ops-eyebrow">Governance</span>
          <h2>Available actions</h2>
        </div>
        {mode === "replay" ? <span className="goal-ops-replay-lock"><Icon name="lock" size={13} /> Disabled in replay</span> : null}
      </div>
      <div className="goal-ops-action-list">
        {actions.length ? actions.map((action) => {
          const disabled = mode === "replay" || !action.enabled || !!pendingActionId;
          const disabledReason = mode === "replay"
            ? "Return to Live mode to perform actions."
            : action.disabled_reason;
          return (
            <div className="goal-ops-action-row" key={action.id}>
              <div>
                <strong>{action.label}</strong>
                <span>{disabledReason || action.impact || `${action.kind.replaceAll("_", " ")} action`}</span>
              </div>
              <button
                type="button"
                className={action.destructive ? "goal-ops-action destructive" : "goal-ops-action"}
                disabled={disabled}
                aria-describedby={`action-description-${action.id}`}
                onClick={() => begin(action)}
              >
                {pendingActionId === action.id ? "Working" : action.label}
              </button>
              <span id={`action-description-${action.id}`} className="goal-ops-sr-only">{disabledReason || action.impact}</span>
            </div>
          );
        }) : <p className="goal-ops-muted">No governed actions are available for this goal.</p>}
      </div>
      <ConfirmDialog
        open={selected !== null}
        title={selected?.label || "Confirm action"}
        message={selected?.impact || "This action changes the live goal."}
        confirmLabel={selected?.label || "Confirm"}
        confirmTone={selected?.destructive ? "danger" : "default"}
        confirmDisabled={
          pendingActionId === selected?.id ||
          (!!selected?.reason_required && !reason.trim()) ||
          !!invalidPayload
        }
        onCancel={() => setSelected(null)}
        onConfirm={() => void submit()}
      >
        <fieldset className="goal-ops-dialog-fields">
          <legend className="goal-ops-sr-only">{selected?.label || "Action"} details</legend>
          {payloadFields.map((field) => {
            const fieldOptions = payloadOptions(field.options);
            return (
              <label className="goal-ops-dialog-field" key={field.name}>
                <span>{field.label}{field.required ? " (required)" : ""}</span>
                {fieldOptions.length ? (
                  <select
                    value={payload[field.name] || ""}
                    required={field.required}
                    onChange={(event) => setPayloadField(field, event.currentTarget.value)}
                  >
                    {!field.required ? <option value="">Not specified</option> : null}
                    {fieldOptions.map((option) => (
                      <option value={option.value} key={option.value}>{option.label}</option>
                    ))}
                  </select>
                ) : field.format === "json" ? (
                  <textarea
                    value={payload[field.name] || ""}
                    required={field.required}
                    aria-invalid={!!payload[field.name] && !!invalidPayload}
                    onChange={(event) => setPayloadField(field, event.currentTarget.value)}
                  />
                ) : (
                  <input
                    type="text"
                    value={payload[field.name] || ""}
                    required={field.required}
                    onChange={(event) => setPayloadField(field, event.currentTarget.value)}
                  />
                )}
              </label>
            );
          })}
          {invalidPayload ? <span className="goal-ops-field-error" role="alert">{invalidPayload}</span> : null}
          {decisionOptions.length ? (
            <label className="goal-ops-dialog-field">
              <span>Decision</span>
              <select value={decision} onChange={(event) => setDecision(event.currentTarget.value)}>
                {decisionOptions.map((option) => <option value={option.value} key={option.value}>{option.label}</option>)}
              </select>
            </label>
          ) : null}
          {selected?.reason_required || selected?.destructive ? (
            <label className="goal-ops-dialog-field">
              <span>Reason{selected.reason_required ? " (required)" : ""}</span>
              <textarea value={reason} required={selected.reason_required} onChange={(event) => setReason(event.currentTarget.value)} />
            </label>
          ) : null}
        </fieldset>
      </ConfirmDialog>
    </section>
  );
}
