import { useMemo, useState } from "react";

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

type LazyJsonProps = {
  value: unknown;
  /** Visible label for the toggle. */
  label?: string;
  className?: string;
  preClassName?: string;
  /** Render hint shown next to the toggle, e.g. "12 KB" */
  hint?: string;
};

/**
 * Render a JSON payload behind a `<details>` toggle, deferring stringification
 * until the toggle is opened. Big payloads (run/blackboard/event) can be 100+ KB
 * and are otherwise serialized on every render even when collapsed in the DOM.
 */
export function LazyJson({
  value,
  label = "Show raw JSON",
  className = "",
  preClassName = "tcp-code mt-2 max-h-64 overflow-auto text-[11px]",
  hint,
}: LazyJsonProps) {
  const [opened, setOpened] = useState(false);
  const text = useMemo(() => (opened ? safeStringify(value) : ""), [opened, value]);
  return (
    <details
      className={className}
      onToggle={(event) => setOpened((event.currentTarget as HTMLDetailsElement).open)}
    >
      <summary className="cursor-pointer select-none text-xs text-slate-400 hover:text-slate-200">
        {label}
        {hint ? <span className="ml-2 text-[10px] text-slate-500">{hint}</span> : null}
      </summary>
      {opened ? <pre className={preClassName}>{text}</pre> : null}
    </details>
  );
}

/**
 * Pre-formatted JSON behind an external open flag (for cases where the parent
 * already owns a `<details>` and just wants the body to be lazy).
 */
export function DeferredJson({
  value,
  open,
  className = "tcp-code mt-2 max-h-32 overflow-auto text-[11px]",
}: {
  value: unknown;
  open: boolean;
  className?: string;
}) {
  const text = useMemo(() => (open ? safeStringify(value) : ""), [open, value]);
  if (!open) return null;
  return <pre className={className}>{text}</pre>;
}
