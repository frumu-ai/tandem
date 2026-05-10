import type { ExecutionProfile } from "./AutomationsRunHelpers";

type ProfileValue = ExecutionProfile | "";

interface Segment {
  value: ExecutionProfile;
  label: string;
  tooltip: string;
  activeClass: string;
}

const SEGMENTS: Segment[] = [
  {
    value: "strict",
    label: "Strict",
    tooltip:
      "All validators enforced. Use Guided/YOLO during validator hardening to unblock recoverable runs.",
    activeClass: "bg-emerald-500/20 text-emerald-100 ring-emerald-400/40",
  },
  {
    value: "guided",
    label: "Guided",
    tooltip:
      "Non-critical validation failures become warnings; critical failures (auth, secret access, destructive-action approval, budget caps) still block.",
    activeClass: "bg-amber-400/20 text-amber-100 ring-amber-400/40",
  },
  {
    value: "yolo",
    label: "YOLO",
    tooltip:
      "Non-critical validation failures continue as experimental; spend caps and approvals still enforced. Downstream nodes inherit the experimental flag.",
    activeClass: "bg-rose-500/20 text-rose-100 ring-rose-400/40",
  },
];

export function ExecutionProfileToggle({
  value,
  onChange,
  disabled = false,
  pendingValue = null,
  size = "md",
  clearable = false,
  className = "",
}: {
  value: ProfileValue;
  onChange: (next: ProfileValue) => void;
  disabled?: boolean;
  /** Set to one of the three values to render that segment as "Saving…". */
  pendingValue?: ExecutionProfile | null;
  size?: "sm" | "md";
  /** When true and value !== "", render a "Use system default" link below. */
  clearable?: boolean;
  className?: string;
}) {
  const sizeClass = size === "sm" ? "h-6 px-2 text-[10px]" : "h-7 px-3 text-[11px]";
  return (
    <div className={`grid gap-1 ${className}`}>
      <div
        role="radiogroup"
        aria-label="Execution profile"
        className="inline-flex w-fit items-center rounded-full border border-slate-700/60 bg-slate-950/40 p-0.5"
      >
        {SEGMENTS.map((segment) => {
          const isActive = value === segment.value;
          const isPending = pendingValue === segment.value;
          return (
            <div key={segment.value} className="group relative">
              <button
                type="button"
                role="radio"
                aria-checked={isActive}
                disabled={disabled}
                className={`${sizeClass} rounded-full font-semibold uppercase tracking-wide transition focus:outline-none focus:ring-2 focus:ring-sky-400/60 ${
                  isActive
                    ? `ring-1 ${segment.activeClass}`
                    : "text-slate-300 hover:bg-slate-800/60 hover:text-slate-100"
                } ${disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer"}`}
                onClick={() => {
                  if (disabled) return;
                  onChange(segment.value);
                }}
              >
                {isPending ? "Saving…" : segment.label}
              </button>
              <div
                role="tooltip"
                className="pointer-events-none absolute left-1/2 top-full z-20 mt-1.5 w-56 -translate-x-1/2 rounded-md border border-slate-700/70 bg-slate-950/95 p-2 text-[11px] leading-snug text-slate-200 opacity-0 shadow-lg transition group-hover:opacity-100 group-focus-within:opacity-100"
              >
                <div className="mb-0.5 font-semibold text-slate-100">{segment.label}</div>
                {segment.tooltip}
              </div>
            </div>
          );
        })}
      </div>
      {clearable && value !== "" ? (
        <button
          type="button"
          className="w-fit text-[11px] text-slate-400 underline-offset-2 hover:text-slate-200 hover:underline"
          disabled={disabled}
          onClick={() => onChange("")}
        >
          Use system default
        </button>
      ) : null}
    </div>
  );
}
