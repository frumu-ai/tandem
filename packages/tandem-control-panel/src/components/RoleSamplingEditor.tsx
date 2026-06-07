import { useEffect, useRef, useState } from "react";
import {
  SAMPLING_FIELDS,
  SWARM_ROLE_KEYS,
  applyRoleSampling,
  readRoleSampling,
} from "../pages/roleSampling.js";

type Drafts = Record<string, Record<string, string>>;

type RoleSamplingEditorProps = {
  /** The current install config JSON text (single source of truth). */
  configText: string;
  /** Commit an updated config JSON text. */
  onChange: (next: string) => void;
};

const ROLE_LABELS: Record<string, string> = {
  manager: "Manager",
  worker: "Worker",
  reviewer: "Reviewer",
  tester: "Tester",
};

/**
 * Optional per-role sampling controls (temperature / top_p / max_tokens) for the
 * manager / worker / reviewer / tester roles. Edits the same install config JSON
 * the textarea below shows, so values persist through the existing save path and
 * flow to ACA exactly like the per-role provider/model selection.
 *
 * Empty input = unset (engine default); blanks are never written as a value.
 */
export function RoleSamplingEditor({ configText, onChange }: RoleSamplingEditorProps) {
  const initial = readRoleSampling(configText);
  const [drafts, setDrafts] = useState<Drafts>(initial.values);
  const [fieldError, setFieldError] = useState<string>("");
  // Tracks text this component wrote, so the echo from `onChange` does not
  // clobber in-progress typing while still re-syncing on external edits.
  const lastWrittenRef = useRef<string>(configText);

  useEffect(() => {
    if (configText === lastWrittenRef.current) return;
    lastWrittenRef.current = configText;
    setDrafts(readRoleSampling(configText).values);
    setFieldError("");
  }, [configText]);

  const parsed = readRoleSampling(configText);
  const disabled = !parsed.ok;

  const handleInput = (role: string, fieldKey: string, value: string) => {
    setDrafts((prev) => ({
      ...prev,
      [role]: { ...(prev[role] || {}), [fieldKey]: value },
    }));
    const result = applyRoleSampling(configText, role, fieldKey, value);
    if (result.ok) {
      setFieldError("");
      lastWrittenRef.current = result.text;
      onChange(result.text);
    } else {
      setFieldError(`${ROLE_LABELS[role] || role} · ${result.error}`);
    }
  };

  return (
    <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
      <div className="font-medium">Per-role sampling</div>
      <div className="tcp-subtle mt-1 text-xs">
        Optional sampling overrides per swarm role. Leave a field blank to use the engine default —
        blanks are never written. Lower the temperature for JSON-emitting roles (reviewer / tester)
        to reduce malformed output. Values are clamped per provider by the engine.
      </div>

      {disabled ? (
        <div className="mt-3 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
          Fix the config JSON below to edit per-role sampling.
        </div>
      ) : (
        <div className="mt-3 grid gap-3">
          {SWARM_ROLE_KEYS.map((role) => (
            <div
              key={role}
              className="grid grid-cols-1 gap-2 sm:grid-cols-[6rem_repeat(3,minmax(0,1fr))] sm:items-center"
            >
              <span className="text-sm font-medium">{ROLE_LABELS[role] || role}</span>
              {SAMPLING_FIELDS.map((field) => (
                <label key={field.key} className="grid gap-1">
                  <span className="tcp-subtle text-[11px]">{field.label}</span>
                  <input
                    className="tcp-input h-9 w-full text-xs"
                    type="number"
                    inputMode="decimal"
                    step={field.step}
                    placeholder={field.placeholder}
                    value={drafts[role]?.[field.key] ?? ""}
                    onInput={(event) =>
                      handleInput(role, field.key, (event.target as HTMLInputElement).value)
                    }
                  />
                </label>
              ))}
            </div>
          ))}
        </div>
      )}

      {fieldError ? (
        <div className="mt-3 rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-200">
          {fieldError}
        </div>
      ) : null}
    </div>
  );
}
