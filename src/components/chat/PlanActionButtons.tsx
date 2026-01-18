import { useState } from "react";
import { Play, RotateCcw, X, ListTodo } from "lucide-react";
import { cn } from "@/lib/utils";

interface PlanActionButtonsProps {
  onImplement: () => void;
  onRework: (feedback: string) => void;
  onCancel: () => void;
  onViewTasks?: () => void;
  disabled?: boolean;
}

export function PlanActionButtons({
  onImplement,
  onRework,
  onCancel,
  onViewTasks,
  disabled,
}: PlanActionButtonsProps) {
  const [showReworkInput, setShowReworkInput] = useState(false);
  const [reworkFeedback, setReworkFeedback] = useState("");

  const handleRework = () => {
    if (showReworkInput && reworkFeedback.trim()) {
      onRework(reworkFeedback);
      setShowReworkInput(false);
      setReworkFeedback("");
    } else {
      setShowReworkInput(true);
    }
  };

  return (
    <div className="mt-3 flex flex-col gap-2">
      {showReworkInput ? (
        <div className="flex flex-col gap-2">
          <textarea
            value={reworkFeedback}
            onChange={(e) => setReworkFeedback(e.target.value)}
            placeholder="What should I change about this plan?"
            className="w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text placeholder:text-text-muted resize-none focus:outline-none focus:ring-2 focus:ring-primary"
            rows={3}
            autoFocus
          />
          <div className="flex gap-2">
            <button
              onClick={handleRework}
              disabled={!reworkFeedback.trim() || disabled}
              className="flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <RotateCcw className="h-4 w-4" />
              Send Feedback
            </button>
            <button
              onClick={() => {
                setShowReworkInput(false);
                setReworkFeedback("");
              }}
              className="rounded-md border border-border px-4 py-2 text-sm font-medium text-text-muted transition-colors hover:bg-surface-elevated"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <button
              onClick={onImplement}
              disabled={disabled}
              className={cn(
                "flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium transition-colors",
                "bg-primary text-white hover:bg-primary/90",
                "disabled:opacity-50 disabled:cursor-not-allowed"
              )}
              title="Execute this plan"
            >
              <Play className="h-4 w-4" />
              Implement this
            </button>

            <button
              onClick={handleRework}
              disabled={disabled}
              className={cn(
                "flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium transition-colors",
                "border border-border text-text hover:bg-surface-elevated",
                "disabled:opacity-50 disabled:cursor-not-allowed"
              )}
              title="Request changes to the plan"
            >
              <RotateCcw className="h-4 w-4" />
              Rework
            </button>

            <button
              onClick={onCancel}
              disabled={disabled}
              className={cn(
                "flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium transition-colors",
                "text-text-muted hover:text-error hover:bg-error/10",
                "disabled:opacity-50 disabled:cursor-not-allowed"
              )}
              title="Cancel this plan"
            >
              <X className="h-4 w-4" />
              Cancel
            </button>
          </div>

          {onViewTasks && (
            <button
              onClick={onViewTasks}
              className={cn(
                "flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                "border border-border text-text hover:bg-surface-elevated"
              )}
              title="View task list"
            >
              <ListTodo className="h-4 w-4" />
              View Tasks
            </button>
          )}
        </div>
      )}
    </div>
  );
}
