import { useState } from "react";
import { Icon } from "../../ui/Icon";

type WatchCondition =
  | {
      kind: "handoff_available";
      source_automation_id?: string | null;
      artifact_type?: string | null;
    }
  | { kind: string; [key: string]: unknown };

interface Props {
  value: WatchCondition[];
  onChange: (next: WatchCondition[]) => void;
  automations?: { automation_id?: string; id?: string; name?: string }[];
}

function blank(): WatchCondition {
  return { kind: "handoff_available", source_automation_id: null, artifact_type: null };
}

export function WatchConditionEditor({ value, onChange, automations = [] }: Props) {
  const [adding, setAdding] = useState(false);

  const update = (index: number, patch: Partial<WatchCondition>) => {
    onChange(value.map((cond, i) => (i === index ? { ...cond, ...patch } : cond)));
  };

  const remove = (index: number) => {
    onChange(value.filter((_, i) => i !== index));
  };

  const add = () => {
    onChange([...value, blank()]);
    setAdding(false);
  };

  return (
    <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
      <div className="flex items-center justify-between gap-2">
        <div>
          <div className="text-xs uppercase tracking-wide text-slate-500">Watch conditions</div>
          <div className="mt-0.5 text-xs text-slate-500">
            Trigger this automation when specific handoffs become available.
          </div>
        </div>
        {value.length > 0 && (
          <span className="rounded-full bg-amber-400/15 px-2 py-0.5 text-xs font-medium text-amber-400">
            ⚡ watch-triggered
          </span>
        )}
      </div>

      {value.length === 0 && (
        <div className="rounded-lg border border-dashed border-slate-700/40 p-3 text-center text-xs text-slate-500">
          No watch conditions — this automation is scheduled or manual only.
        </div>
      )}

      {value.map((cond, i) => (
        <div
          key={i}
          className="grid gap-2 rounded-lg border border-amber-400/20 bg-amber-400/5 p-3"
        >
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Icon name="zap" className="h-3.5 w-3.5 text-amber-400" />
              <span className="text-xs font-medium text-amber-300">HandoffAvailable</span>
            </div>
            <button
              type="button"
              className="tcp-btn h-6 w-6 px-0 text-red-400/70 hover:text-red-300"
              onClick={() => remove(i)}
              title="Remove condition"
            >
              <Icon name="x" />
            </button>
          </div>

          <div className="grid gap-2 sm:grid-cols-2">
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Source automation</label>
              <select
                className="tcp-select text-xs"
                value={(cond as any).source_automation_id ?? ""}
                onChange={(e) =>
                  update(i, {
                    source_automation_id: e.target.value || null,
                  } as any)
                }
              >
                <option value="">Any automation</option>
                {automations.map((a) => {
                  const id = a.automation_id ?? a.id ?? "";
                  return (
                    <option key={id} value={id}>
                      {a.name ?? id}
                    </option>
                  );
                })}
              </select>
            </div>

            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Artifact type filter</label>
              <input
                className="tcp-input text-xs"
                placeholder="e.g. shortlist, brief, report"
                value={(cond as any).artifact_type ?? ""}
                onInput={(e) =>
                  update(i, {
                    artifact_type: (e.target as HTMLInputElement).value || null,
                  } as any)
                }
              />
            </div>
          </div>
        </div>
      ))}

      {adding ? (
        <div className="flex gap-2">
          <button type="button" className="tcp-btn flex-1" onClick={add}>
            <Icon name="zap" />
            Add HandoffAvailable condition
          </button>
          <button type="button" className="tcp-btn" onClick={() => setAdding(false)}>
            <Icon name="x" />
          </button>
        </div>
      ) : (
        <button type="button" className="tcp-btn w-full text-xs" onClick={() => setAdding(true)}>
          <Icon name="plus" />
          Add watch condition
        </button>
      )}
    </div>
  );
}
