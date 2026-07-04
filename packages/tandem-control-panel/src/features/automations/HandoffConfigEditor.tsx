import { Icon } from "../../ui/Icon";
interface HandoffConfig {
  inbox_dir?: string | null;
  approved_dir?: string | null;
  archived_dir?: string | null;
  auto_approve?: boolean | null;
}

const DEFAULTS: Required<HandoffConfig> = {
  inbox_dir: "shared/handoffs/inbox",
  approved_dir: "shared/handoffs/approved",
  archived_dir: "shared/handoffs/archived",
  auto_approve: true,
};

interface Props {
  value: HandoffConfig | null | undefined;
  onChange: (next: HandoffConfig) => void;
}

export function HandoffConfigEditor({ value, onChange }: Props) {
  const cfg: Required<HandoffConfig> = {
    inbox_dir: value?.inbox_dir ?? DEFAULTS.inbox_dir,
    approved_dir: value?.approved_dir ?? DEFAULTS.approved_dir,
    archived_dir: value?.archived_dir ?? DEFAULTS.archived_dir,
    auto_approve: value?.auto_approve ?? DEFAULTS.auto_approve,
  };

  const update = (patch: Partial<HandoffConfig>) => onChange({ ...cfg, ...patch });
  const reset = () => onChange({ ...DEFAULTS });

  return (
    <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
      <div className="flex items-center justify-between gap-2">
        <div>
          <div className="text-xs uppercase tracking-wide text-slate-500">Handoff config</div>
          <div className="mt-0.5 text-xs text-slate-500">
            Directory layout for handoff artifacts. Relative to workspace root.
          </div>
        </div>
        <button
          type="button"
          className="tcp-btn h-7 px-2 text-xs text-slate-400"
          onClick={reset}
          title="Reset to defaults"
        >
          <Icon name="rotate-ccw" />
          Reset
        </button>
      </div>

      {/* auto_approve toggle */}
      <button
        type="button"
        className={`tcp-list-item flex h-10 items-center justify-between px-3 text-left text-xs ${
          cfg.auto_approve
            ? "border-emerald-400/60 bg-emerald-400/10"
            : "border-amber-400/60 bg-amber-400/10"
        }`}
        role="switch"
        aria-checked={!!cfg.auto_approve}
        onClick={() => update({ auto_approve: !cfg.auto_approve })}
      >
        <span className="flex items-center gap-2">
          <Icon name={cfg.auto_approve ? "zap" : "lock"} className="h-3.5 w-3.5" />
          {cfg.auto_approve
            ? "Auto-approve — handoffs move directly to approved/"
            : "Manual approval — handoffs wait in inbox/ until reviewed"}
        </span>
        <span
          className={`relative h-5 w-9 rounded-full transition ${
            cfg.auto_approve ? "bg-emerald-500/40" : "bg-amber-500/40"
          }`}
        >
          <span
            className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-slate-100 transition ${
              cfg.auto_approve ? "translate-x-4" : ""
            }`}
          />
        </span>
      </button>

      <div className="grid gap-2">
        {(
          [
            { key: "inbox_dir", label: "Inbox directory", color: "text-amber-400" },
            { key: "approved_dir", label: "Approved directory", color: "text-emerald-400" },
            { key: "archived_dir", label: "Archived directory", color: "text-slate-400" },
          ] as const
        ).map(({ key, label, color }) => (
          <div key={key} className="grid gap-1">
            <label className={`text-xs ${color}`}>{label}</label>
            <input
              className="tcp-input font-mono text-xs"
              value={cfg[key] ?? ""}
              onInput={(e) => update({ [key]: (e.target as HTMLInputElement).value || null })}
              placeholder={DEFAULTS[key]}
            />
          </div>
        ))}
      </div>
    </div>
  );
}
