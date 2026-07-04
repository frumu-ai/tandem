import { Icon } from "../../ui/Icon";
interface ScopePolicy {
  readable_paths?: string[] | null;
  writable_paths?: string[] | null;
  denied_paths?: string[] | null;
  watch_paths?: string[] | null;
}

interface Props {
  value: ScopePolicy | null | undefined;
  onChange: (next: ScopePolicy | null) => void;
}

function pathsToText(paths: string[] | null | undefined): string {
  return (paths ?? []).join("\n");
}

function textToPaths(text: string): string[] | null {
  const lines = text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  return lines.length > 0 ? lines : null;
}

function isOpen(policy: ScopePolicy | null | undefined): boolean {
  if (!policy) return true;
  const { readable_paths, writable_paths, denied_paths, watch_paths } = policy;
  return (
    (!readable_paths || readable_paths.length === 0) &&
    (!writable_paths || writable_paths.length === 0) &&
    (!denied_paths || denied_paths.length === 0) &&
    (!watch_paths || watch_paths.length === 0)
  );
}

export function ScopePolicyEditor({ value, onChange }: Props) {
  const open = isOpen(value);

  const update = (key: keyof ScopePolicy, text: string) => {
    const paths = textToPaths(text);
    const next: ScopePolicy = {
      ...(value ?? {}),
      [key]: paths,
    };
    // If all are empty, set null (open policy)
    if (isOpen(next)) {
      onChange(null);
    } else {
      onChange(next);
    }
  };

  const clear = () => onChange(null);

  return (
    <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-4">
      <div className="flex items-center justify-between gap-2">
        <div>
          <div className="text-xs uppercase tracking-wide text-slate-500">Scope policy</div>
          <div className="mt-0.5 text-xs text-slate-500">
            Filesystem sandbox for agents in this automation. Empty = fully open.
          </div>
        </div>
        {!open && (
          <button
            type="button"
            className="tcp-btn h-7 px-2 text-xs text-slate-400"
            onClick={clear}
            title="Clear all paths (open policy)"
          >
            <Icon name="shield-off" />
            Clear
          </button>
        )}
      </div>

      {open ? (
        <div className="rounded-lg border border-dashed border-slate-700/40 p-3 text-center">
          <div className="text-xs font-medium text-slate-400">Open policy — no restrictions</div>
          <div className="mt-1 text-xs text-slate-500">
            Fill in any path list below to restrict agent filesystem access.
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-1.5">
          {(value?.denied_paths ?? []).length > 0 && (
            <span className="rounded-full border border-red-400/30 bg-red-400/10 px-2 py-0.5 text-xs text-red-400">
              {value?.denied_paths?.length} denied
            </span>
          )}
          {(value?.readable_paths ?? []).length > 0 && (
            <span className="rounded-full border border-sky-400/30 bg-sky-400/10 px-2 py-0.5 text-xs text-sky-400">
              {value?.readable_paths?.length} readable
            </span>
          )}
          {(value?.writable_paths ?? []).length > 0 && (
            <span className="rounded-full border border-amber-400/30 bg-amber-400/10 px-2 py-0.5 text-xs text-amber-400">
              {value?.writable_paths?.length} writable
            </span>
          )}
          {(value?.watch_paths ?? []).length > 0 && (
            <span className="rounded-full border border-violet-400/30 bg-violet-400/10 px-2 py-0.5 text-xs text-violet-400">
              {value?.watch_paths?.length} watch
            </span>
          )}
        </div>
      )}

      <div className="grid gap-3">
        <PathField
          label="Readable paths"
          hint="Agents may read these paths. Empty = all paths readable."
          color="sky"
          value={pathsToText(value?.readable_paths)}
          onChange={(t) => update("readable_paths", t)}
        />
        <PathField
          label="Writable paths"
          hint="Agents may write to these paths. Must be a subset of readable."
          color="amber"
          value={pathsToText(value?.writable_paths)}
          onChange={(t) => update("writable_paths", t)}
        />
        <PathField
          label="Denied paths"
          hint="Always blocked, even if listed in readable/writable. Takes priority."
          color="red"
          value={pathsToText(value?.denied_paths)}
          onChange={(t) => update("denied_paths", t)}
        />
        <PathField
          label="Watch paths"
          hint="Paths the watch evaluator may scan. Defaults to readable_paths if empty."
          color="violet"
          value={pathsToText(value?.watch_paths)}
          onChange={(t) => update("watch_paths", t)}
        />
      </div>
    </div>
  );
}

function PathField({
  label,
  hint,
  color,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  color: "sky" | "amber" | "red" | "violet";
  value: string;
  onChange: (t: string) => void;
}) {
  const borderColor =
    color === "sky"
      ? "focus-within:border-sky-400/60"
      : color === "amber"
        ? "focus-within:border-amber-400/60"
        : color === "red"
          ? "focus-within:border-red-400/60"
          : "focus-within:border-violet-400/60";

  return (
    <div className="grid gap-1">
      <label className="text-xs text-slate-400">{label}</label>
      <textarea
        className={`tcp-input min-h-[72px] font-mono text-xs leading-5 transition-colors ${borderColor}`}
        value={value}
        placeholder={"shared/handoffs/\njob-search/reports/"}
        onInput={(e) => onChange((e.target as HTMLTextAreaElement).value)}
      />
      <div className="text-xs text-slate-600">{hint} One path per line, prefix matching.</div>
    </div>
  );
}
