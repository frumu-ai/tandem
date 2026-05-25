import { Repeat } from "lucide-react";
import { cn } from "@/lib/utils";

interface LoopToggleProps {
  enabled: boolean;
  onToggle: (enabled: boolean) => void;
  disabled?: boolean;
}

export function LoopToggle({ enabled, onToggle, disabled }: LoopToggleProps) {
  return (
    <button
      type="button"
      onClick={() => onToggle(!enabled)}
      disabled={disabled}
      className={cn(
        "flex items-center gap-1 rounded-md border px-2 py-1 text-[10px] transition-colors",
        enabled
          ? "border-amber-500/40 bg-amber-500/15 text-amber-200"
          : "border-border bg-surface-elevated text-text-subtle hover:text-text",
        disabled && "cursor-not-allowed opacity-60"
      )}
      title="Ralph Loop: An autonomous agent that iterates on complex tasks until completion. Consumes more credits and takes longer, but produces higher quality results."
    >
      <Repeat className="h-3 w-3" />
      <span>Loop</span>
    </button>
  );
}
