import { useRef, useState } from "react";
import type { ExecutionProfile } from "./AutomationsRunHelpers";

type ProfileValue = ExecutionProfile | "";

interface Segment {
  value: ExecutionProfile;
  label: string;
  tooltip: string;
  /** Tailwind text/glow color for the active dot. */
  accentClass: string;
}

const SEGMENTS: Segment[] = [
  {
    value: "strict",
    label: "Strict",
    tooltip:
      "All validators enforced. Use Guided/YOLO during validator hardening to unblock recoverable runs.",
    accentClass: "bg-emerald-300 shadow-[0_0_0_3px_rgba(110,231,183,0.18)]",
  },
  {
    value: "guided",
    label: "Guided",
    tooltip:
      "Non-critical validation failures become warnings; critical failures (auth, secret access, destructive-action approval, budget caps) still block.",
    accentClass: "bg-amber-300 shadow-[0_0_0_3px_rgba(252,211,77,0.18)]",
  },
  {
    value: "yolo",
    label: "YOLO",
    tooltip:
      "Non-critical validation failures continue as experimental; spend caps and approvals still enforced. Downstream nodes inherit the experimental flag.",
    accentClass: "bg-rose-300 shadow-[0_0_0_3px_rgba(253,164,175,0.18)]",
  },
];

function segmentFor(value: ProfileValue): Segment | null {
  return SEGMENTS.find((s) => s.value === value) || null;
}

export function ExecutionProfileToggle({
  value,
  onChange,
  disabled = false,
  pendingValue = null,
  clearable = false,
  className = "",
}: {
  value: ProfileValue;
  onChange: (next: ProfileValue) => void;
  disabled?: boolean;
  pendingValue?: ExecutionProfile | null;
  clearable?: boolean;
  className?: string;
}) {
  const [hoverProfile, setHoverProfile] = useState<ExecutionProfile | null>(null);
  const [tooltipPos, setTooltipPos] = useState<{ top: number; left: number } | null>(null);
  const buttonRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const activeSegment = segmentFor(value);

  const handleEnter = (profile: ExecutionProfile) => {
    const el = buttonRefs.current[profile];
    if (el) {
      const rect = el.getBoundingClientRect();
      setTooltipPos({
        top: rect.bottom + 8,
        left: rect.left + rect.width / 2,
      });
    }
    setHoverProfile(profile);
  };

  const handleLeave = () => {
    setHoverProfile(null);
    setTooltipPos(null);
  };

  const hoverSegment = hoverProfile ? segmentFor(hoverProfile) : null;

  return (
    <div className={`grid gap-1.5 ${className}`}>
      <div className="flex items-center gap-3">
        <div
          role="radiogroup"
          aria-label="Execution profile"
          className="inline-flex items-center gap-1 rounded-full border border-slate-700/70 bg-slate-900/70 px-2 py-1.5"
        >
          {SEGMENTS.map((segment) => {
            const isActive = value === segment.value;
            const isPending = pendingValue === segment.value;
            return (
              <button
                key={segment.value}
                ref={(el) => {
                  buttonRefs.current[segment.value] = el;
                }}
                type="button"
                role="radio"
                aria-checked={isActive}
                aria-label={segment.label}
                disabled={disabled}
                className={`flex items-center justify-center rounded-full transition focus:outline-none focus:ring-2 focus:ring-sky-400/60 ${
                  disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer"
                }`}
                style={{ width: 18, height: 18 }}
                onMouseEnter={() => handleEnter(segment.value)}
                onMouseLeave={handleLeave}
                onFocus={() => handleEnter(segment.value)}
                onBlur={handleLeave}
                onClick={() => {
                  if (disabled) return;
                  onChange(segment.value);
                }}
              >
                <span
                  className={`block rounded-full transition ${
                    isActive
                      ? `h-3 w-3 ${segment.accentClass}`
                      : isPending
                        ? "h-2 w-2 animate-pulse bg-slate-300"
                        : "h-1.5 w-1.5 bg-slate-500 hover:bg-slate-300"
                  }`}
                />
              </button>
            );
          })}
        </div>
        <div className="grid gap-0.5 text-left">
          <div className="text-xs font-semibold uppercase tracking-wide text-slate-100">
            {activeSegment ? activeSegment.label : "System default"}
          </div>
          <div className="text-[11px] text-slate-400">
            {activeSegment ? "" : "Falls back to tenant default · Strict if none set"}
          </div>
        </div>
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

      {hoverSegment && tooltipPos ? (
        <div
          role="tooltip"
          className="pointer-events-none fixed z-[1000] w-64 -translate-x-1/2 rounded-md border border-slate-700/70 bg-slate-950/95 p-2.5 text-[11px] leading-snug text-slate-200 shadow-xl"
          style={{ top: tooltipPos.top, left: tooltipPos.left }}
        >
          <div className="mb-1 font-semibold uppercase tracking-wide text-slate-100">
            {hoverSegment.label}
          </div>
          {hoverSegment.tooltip}
        </div>
      ) : null}
    </div>
  );
}
