import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { ChevronDown, ChevronRight, X, ListTodo, Play } from "lucide-react";
import { type TodoItem } from "@/lib/tauri";
import { TaskItem } from "./TaskItem";
import { cn } from "@/lib/utils";

interface TaskSidebarProps {
  isOpen: boolean;
  onClose: () => void;
  todos: TodoItem[];
  pending: TodoItem[];
  inProgress: TodoItem[];
  completed: TodoItem[];
  isLoading?: boolean;
  onExecutePending?: () => void;
  isExecuting?: boolean;
}

export function TaskSidebar({
  isOpen,
  onClose,
  todos,
  pending,
  inProgress,
  completed,
  isLoading,
  onExecutePending,
  isExecuting,
}: TaskSidebarProps) {
  const [expandedSections, setExpandedSections] = useState<Set<string>>(
    new Set(["pending", "in_progress"])
  );

  const toggleSection = (section: string) => {
    setExpandedSections((prev) => {
      const next = new Set(prev);
      if (next.has(section)) {
        next.delete(section);
      } else {
        next.add(section);
      }
      return next;
    });
  };

  if (!isOpen) return null;

  const sections = [
    { id: "pending", title: "Pending", items: pending, color: "text-amber-400" },
    { id: "in_progress", title: "In Progress", items: inProgress, color: "text-primary" },
    { id: "completed", title: "Completed", items: completed, color: "text-success" },
  ];

  const hasAnyTodos = todos.length > 0;

  return (
    <AnimatePresence>
      {isOpen && (
        <motion.aside
          initial={{ x: 320 }}
          animate={{ x: 0 }}
          exit={{ x: 320 }}
          transition={{ type: "spring", damping: 25, stiffness: 200 }}
          className="fixed right-0 top-0 bottom-0 w-80 bg-surface border-l border-border shadow-2xl z-30 flex flex-col"
        >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-border">
          <div className="flex items-center gap-2">
            <ListTodo className="h-5 w-5 text-primary" />
            <h2 className="font-semibold text-text">Tasks</h2>
            {todos.length > 0 && (
              <span className="px-2 py-0.5 text-xs bg-primary/10 text-primary rounded-full">
                {todos.length}
              </span>
            )}
          </div>
          <button
            onClick={onClose}
            className="p-1 hover:bg-surface-elevated rounded-md transition-colors"
            title="Close tasks panel"
          >
            <X className="h-5 w-5 text-text-muted" />
          </button>
        </div>

        {/* Execute Pending Button */}
        {pending.length > 0 && onExecutePending && (
          <div className="p-4 border-b border-border">
            <button
              onClick={onExecutePending}
              disabled={isExecuting}
              className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors font-medium text-sm disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isExecuting ? (
                <>
                  <motion.div
                    animate={{ rotate: 360 }}
                    transition={{ duration: 1, repeat: Infinity, ease: "linear" }}
                    className="h-4 w-4 border-2 border-white border-t-transparent rounded-full"
                  />
                  Executing Tasks...
                </>
              ) : (
                <>
                  <Play className="h-4 w-4" />
                  Execute {pending.length} Pending Task{pending.length !== 1 ? 's' : ''}
                </>
              )}
            </button>
          </div>
        )}

        {/* Content */}
        <div className="flex-1 overflow-y-auto">
          {isLoading ? (
            <div className="flex items-center justify-center h-32">
              <p className="text-sm text-text-muted">Loading tasks...</p>
            </div>
          ) : !hasAnyTodos ? (
            <div className="flex flex-col items-center justify-center h-32 px-4 text-center">
              <ListTodo className="h-8 w-8 text-text-muted mb-2 opacity-50" />
              <p className="text-sm text-text-muted">
                No tasks yet. The AI will create tasks when planning complex operations.
              </p>
            </div>
          ) : (
            <div className="p-4 space-y-4">
              {sections.map((section) => {
                const isExpanded = expandedSections.has(section.id);
                const hasItems = section.items.length > 0;

                if (!hasItems) return null;

                return (
                  <div key={section.id} className="space-y-2">
                    <button
                      onClick={() => toggleSection(section.id)}
                      className="flex items-center gap-2 w-full hover:bg-surface-elevated p-2 rounded-md transition-colors"
                    >
                      {isExpanded ? (
                        <ChevronDown className="h-4 w-4 text-text-muted" />
                      ) : (
                        <ChevronRight className="h-4 w-4 text-text-muted" />
                      )}
                      <h3 className={cn("text-sm font-medium", section.color)}>
                        {section.title}
                      </h3>
                      <span className="text-xs text-text-muted ml-auto">
                        {section.items.length}
                      </span>
                    </button>

                    {isExpanded && (
                      <div className="space-y-1 ml-2">
                        {section.items.map((item) => (
                          <TaskItem key={item.id} task={item} />
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </motion.aside>
      )}
    </AnimatePresence>
  );
}
