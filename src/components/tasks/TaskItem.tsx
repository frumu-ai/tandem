import { type TodoItem } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { CheckCircle2, Circle, XCircle, Loader2 } from "lucide-react";

interface TaskItemProps {
  task: TodoItem;
}

export function TaskItem({ task }: TaskItemProps) {
  const getIcon = () => {
    switch (task.status) {
      case "completed":
        return <CheckCircle2 className="h-4 w-4 text-success" />;
      case "in_progress":
        return <Loader2 className="h-4 w-4 text-primary animate-spin" />;
      case "cancelled":
        return <XCircle className="h-4 w-4 text-error" />;
      default:
        return <Circle className="h-4 w-4 text-text-muted" />;
    }
  };

  const getTextStyle = () => {
    if (task.status === "completed") {
      return "text-text-muted line-through";
    }
    if (task.status === "cancelled") {
      return "text-error line-through opacity-60";
    }
    return "text-text";
  };

  return (
    <div className="flex items-start gap-2 p-2 rounded-md hover:bg-surface-elevated transition-colors">
      <div className="mt-0.5">{getIcon()}</div>
      <p className={cn("text-sm flex-1", getTextStyle())}>{task.content}</p>
    </div>
  );
}
