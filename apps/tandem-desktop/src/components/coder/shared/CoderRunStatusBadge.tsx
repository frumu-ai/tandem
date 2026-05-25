import {
  AlertCircle,
  CheckCircle2,
  CircleDashed,
  Loader2,
  PauseCircle,
  XCircle,
} from "lucide-react";
import type { AutomationV2RunRecord } from "@/lib/tauri";
import { runStatusTone, type RunStatusTone } from "./coderRunUtils";

type Size = "sm" | "md";

type CoderRunStatusBadgeProps = {
  run: AutomationV2RunRecord | null;
  size?: Size;
  className?: string;
};

const TONE_STYLES: Record<
  RunStatusTone,
  { label: string; chip: string; icon: React.ReactNode; pulse?: boolean }
> = {
  running: {
    label: "Running",
    chip: "border-primary/40 bg-primary/15 text-primary",
    icon: <Loader2 className="h-3 w-3 animate-spin" aria-hidden />,
    pulse: true,
  },
  queued: {
    label: "Queued",
    chip: "border-primary/30 bg-primary/10 text-primary",
    icon: <CircleDashed className="h-3 w-3 animate-pulse" aria-hidden />,
  },
  paused: {
    label: "Paused",
    chip: "border-amber-400/40 bg-amber-400/10 text-amber-200",
    icon: <PauseCircle className="h-3 w-3" aria-hidden />,
  },
  awaiting: {
    label: "Needs approval",
    chip: "border-amber-300/60 bg-amber-300/15 text-amber-100",
    icon: <AlertCircle className="h-3 w-3" aria-hidden />,
    pulse: true,
  },
  failed: {
    label: "Failed",
    chip: "border-red-500/50 bg-red-500/15 text-red-100",
    icon: <XCircle className="h-3 w-3" aria-hidden />,
  },
  cancelled: {
    label: "Cancelled",
    chip: "border-border bg-surface text-text-muted",
    icon: <XCircle className="h-3 w-3" aria-hidden />,
  },
  completed: {
    label: "Completed",
    chip: "border-emerald-500/40 bg-emerald-500/10 text-emerald-200",
    icon: <CheckCircle2 className="h-3 w-3" aria-hidden />,
  },
  unknown: {
    label: "Unknown",
    chip: "border-border bg-surface text-text-muted",
    icon: <CircleDashed className="h-3 w-3" aria-hidden />,
  },
};

export function CoderRunStatusBadge({
  run,
  size = "sm",
  className = "",
}: CoderRunStatusBadgeProps) {
  const tone = runStatusTone(run);
  const style = TONE_STYLES[tone];
  const sizeClasses = size === "md" ? "px-2.5 py-1 text-xs" : "px-2 py-0.5 text-[11px]";
  const pulseDot = style.pulse ? (
    <span className="relative flex h-1.5 w-1.5">
      <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-current opacity-60" />
      <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-current" />
    </span>
  ) : null;
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full border font-medium ${sizeClasses} ${style.chip} ${className}`}
    >
      {pulseDot ?? style.icon}
      {style.label}
    </span>
  );
}
