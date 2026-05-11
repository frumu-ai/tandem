import { useRef, useState } from "react";
import type { ExecutionProfile } from "./AutomationsRunHelpers";

type ProfileValue = ExecutionProfile | "";

interface Segment {
  value: ProfileValue;
  label: string;
  tooltip: string;
  /** Tailwind text/glow color for the active dot. */
  accentClass: string;
}

const SEGMENTS: Segment[] = [
  {
    value: "",
    label: "System default",
    tooltip:
      "Uses the tenant default execution profile. If no tenant default is configured, Tandem runs this workflow as Guided.",
    accentClass: "bg-sky-300 shadow-[0_0_0_3px_rgba(125,211,252,0.2)]",
  },
  {
    value: "strict",
    label: "Strict",
    tooltip:
      "All validators enforced. Use Guided/Lenient during validator hardening to unblock recoverable runs.",
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
    label: "Lenient",
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
  size = "md",
  showGuidance = true,
}: {
  value: ProfileValue;
  onChange: (next: ProfileValue) => void;
  disabled?: boolean;
  pendingValue?: ExecutionProfile | null;
  clearable?: boolean;
  className?: string;
  size?: "sm" | "md";
  showGuidance?: boolean;
}) {
  const [hoverProfile, setHoverProfile] = useState<ProfileValue | null>(null);
  const [tooltipPos, setTooltipPos] = useState<{ top: number; left: number } | null>(null);
  const buttonRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const normalizedValue = segmentFor(value) ? value : "";
  const activeSegment = segmentFor(normalizedValue);
  const dotButtonSize = size === "sm" ? 16 : 22;
  const activeDotSize = size === "sm" ? "h-2.5 w-2.5" : "h-3.5 w-3.5";
  const idleDotSize = size === "sm" ? "h-1.5 w-1.5" : "h-2 w-2";

  const handleEnter = (profile: ProfileValue) => {
    const refKey = profile || "default";
    const el = buttonRefs.current[refKey];
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

  const hoverSegment = hoverProfile !== null ? segmentFor(hoverProfile) : null;

  return (
    <div className={`grid gap-1.5 ${className}`}>
      <div className="flex items-center gap-3">
        <div
          role="radiogroup"
          aria-label="Execution profile"
          className={`relative inline-flex items-center rounded-full border border-sky-500/30 bg-slate-950/80 shadow-[inset_0_0_0_1px_rgba(148,163,184,0.08),0_0_18px_rgba(56,189,248,0.08)] ${
            size === "sm" ? "gap-1 px-1.5 py-1" : "gap-1.5 px-2 py-1.5"
          }`}
        >
          <span className="pointer-events-none absolute left-4 right-4 top-1/2 h-px -translate-y-1/2 bg-gradient-to-r from-sky-500/45 via-slate-500/45 to-rose-400/45" />
          {SEGMENTS.map((segment) => {
            const refKey = segment.value || "default";
            const isActive = normalizedValue === segment.value;
            const isPending = pendingValue === segment.value;
            return (
              <button
                key={segment.value || "default"}
                ref={(el) => {
                  buttonRefs.current[refKey] = el;
                }}
                type="button"
                role="radio"
                aria-checked={isActive}
                aria-label={segment.label}
                disabled={disabled}
                className={`relative z-10 flex items-center justify-center rounded-full border transition focus:outline-none focus:ring-2 focus:ring-sky-400/70 ${
                  isActive
                    ? "border-slate-200/45 bg-slate-800"
                    : "border-slate-600/60 bg-slate-950 hover:border-slate-300/60 hover:bg-slate-800"
                } ${disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer"}`}
                style={{ width: dotButtonSize, height: dotButtonSize }}
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
                      ? `${activeDotSize} ${segment.accentClass}`
                      : isPending
                        ? `${idleDotSize} animate-pulse bg-slate-300`
                        : `${idleDotSize} bg-slate-500 hover:bg-slate-300`
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
            {activeSegment?.value
              ? activeSegment.tooltip
              : "Falls back to tenant default · Guided if none set"}
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

      {showGuidance ? (
        <div className="rounded-md border border-amber-400/20 bg-amber-950/20 px-2.5 py-2 text-[11px] leading-snug text-amber-100/90">
          If workflow runs keep failing on validation or artifact review, try loosening this
          profile: <span className="font-semibold text-amber-100">Guided</span> turns recoverable
          checks into warnings, while <span className="font-semibold text-rose-100">Lenient</span>{" "}
          is the most permissive experimental mode.
        </div>
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
