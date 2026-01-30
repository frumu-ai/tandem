import type { RalphRunStatus } from "./types";

interface LoopStatusChipProps {
  status: RalphRunStatus;
  iteration: number;
  onClick: () => void;
}

const statusLabels: Record<RalphRunStatus, string> = {
  idle: "Idle",
  running: "Running",
  paused: "Paused",
  completed: "Completed",
  cancelled: "Cancelled",
  error: "Error",
};

export function LoopStatusChip({ status, iteration, onClick }: LoopStatusChipProps) {
  const isActive = status === "running" || status === "paused";

  return (
    <button
      onClick={onClick}
      className="flex items-center gap-2 rounded-full bg-amber-500/20 px-3 py-1 text-[11px] text-amber-200 transition-colors hover:bg-amber-500/30"
    >
      {isActive && <span className="h-2 w-2 rounded-full bg-amber-500 animate-pulse" />}
      <span>
        Loop • {statusLabels[status]} • Iter {iteration}
      </span>
    </button>
  );
}
