import { X } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { motion } from "framer-motion";
import { cn } from "@/lib/utils";
import type { Plan } from "@/hooks/usePlans";

interface PlanViewerProps {
  plan: Plan | null;
  onClose: () => void;
}

export function PlanViewer({ plan, onClose }: PlanViewerProps) {
  if (!plan) {
    return null;
  }

  return (
    <motion.div
      className="flex h-full flex-col border-l border-border bg-surface"
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20 }}
      transition={{ duration: 0.2 }}
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border bg-surface-elevated px-4 py-3">
        <div className="flex-1 min-w-0">
          <h2 className="text-sm font-semibold text-text truncate">{plan.fileName}</h2>
          <p className="text-xs text-text-muted truncate">{plan.sessionName}</p>
        </div>
        <button
          onClick={onClose}
          className="ml-2 flex h-8 w-8 items-center justify-center rounded-lg text-text-muted transition-colors hover:bg-surface hover:text-text"
          title="Close plan view"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6">
        <div
          className={cn(
            "prose prose-sm dark:prose-invert max-w-none",
            "prose-headings:text-text prose-p:text-text prose-li:text-text",
            "prose-code:text-primary prose-code:bg-primary/10 prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded",
            "prose-pre:bg-surface-elevated prose-pre:border prose-pre:border-border",
            "prose-a:text-primary hover:prose-a:text-primary/80"
          )}
        >
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{plan.content}</ReactMarkdown>
        </div>
      </div>

      {/* Footer */}
      <div className="border-t border-border bg-surface-elevated px-4 py-2">
        <p className="text-xs text-text-muted">
          Last modified: {plan.lastModified.toLocaleString()}
        </p>
      </div>
    </motion.div>
  );
}
