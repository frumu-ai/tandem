import { useState } from "react";
import { FileText, ChevronDown, ChevronRight, FolderOpen } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";
import type { Plan } from "@/hooks/usePlans";

interface PlanSelectorProps {
  plans: Plan[];
  activePlan: Plan | null;
  onSelectPlan: (plan: Plan) => void;
  onNewPlan?: () => void;
  isLoading?: boolean;
}

export function PlanSelector({
  plans,
  activePlan,
  onSelectPlan,
  onNewPlan,
  isLoading = false,
}: PlanSelectorProps) {
  const [expandedSessions, setExpandedSessions] = useState<Set<string>>(new Set());

  // Group plans by session
  const plansBySession = plans.reduce(
    (acc, plan) => {
      if (!acc[plan.sessionName]) {
        acc[plan.sessionName] = [];
      }
      acc[plan.sessionName].push(plan);
      return acc;
    },
    {} as Record<string, Plan[]>
  );

  const sessionNames = Object.keys(plansBySession).sort();

  const toggleSession = (sessionName: string) => {
    setExpandedSessions((prev) => {
      const next = new Set(prev);
      if (next.has(sessionName)) {
        next.delete(sessionName);
      } else {
        next.add(sessionName);
      }
      return next;
    });
  };

  if (isLoading) {
    return (
      <div className="flex items-center gap-2 text-xs text-text-muted">
        <div className="h-3 w-3 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        Loading plans...
      </div>
    );
  }

  if (plans.length === 0) {
    return (
      <div className="text-xs text-text-muted italic">No plans yet. Ask the AI to create one!</div>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      {sessionNames.map((sessionName) => {
        const sessionPlans = plansBySession[sessionName];
        const isExpanded = expandedSessions.has(sessionName);

        return (
          <div key={sessionName}>
            {/* Session Header */}
            <button
              onClick={() => toggleSession(sessionName)}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs font-medium text-text transition-colors hover:bg-surface-elevated"
            >
              {isExpanded ? (
                <ChevronDown className="h-3 w-3 text-text-muted" />
              ) : (
                <ChevronRight className="h-3 w-3 text-text-muted" />
              )}
              <FolderOpen className="h-3 w-3 text-primary" />
              <span className="flex-1 truncate">{sessionName}</span>
              <span className="text-text-muted">({sessionPlans.length})</span>
            </button>

            {/* Session Plans */}
            <AnimatePresence>
              {isExpanded && (
                <motion.div
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: "auto", opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={{ duration: 0.15 }}
                  className="overflow-hidden"
                >
                  <div className="ml-5 flex flex-col gap-0.5 py-1">
                    {sessionPlans.map((plan) => {
                      const isActive = activePlan?.fullPath === plan.fullPath;

                      return (
                        <button
                          key={plan.fullPath}
                          onClick={() => onSelectPlan(plan)}
                          className={cn(
                            "flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors",
                            isActive
                              ? "bg-primary/20 text-primary font-medium"
                              : "text-text hover:bg-surface-elevated"
                          )}
                        >
                          <FileText className="h-3 w-3 flex-shrink-0" />
                          <span className="flex-1 truncate">{plan.fileName}</span>
                        </button>
                      );
                    })}
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        );
      })}

      {onNewPlan && (
        <div className="mt-2 border-t border-border pt-2">
          <button
            onClick={onNewPlan}
            className="flex w-full items-center justify-center gap-2 rounded-md bg-primary/10 px-2 py-1.5 text-xs font-medium text-primary transition-colors hover:bg-primary/20"
          >
            <FileText className="h-3 w-3" />
            New Plan
          </button>
        </div>
      )}
    </div>
  );
}
