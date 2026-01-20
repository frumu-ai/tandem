import { useState, useRef, useEffect } from "react";
import {
  Wrench,
  Presentation,
  // GitBranch,
  // Table,
  ChevronDown,
  LayoutTemplate,
  Globe,
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";

export interface ToolCategory {
  id: string;
  label: string;
  icon: typeof Presentation;
  description: string;
  defaultEnabled: boolean;
}

const TOOL_CATEGORIES: ToolCategory[] = [
  {
    id: "presentations",
    label: "Slides",
    icon: Presentation,
    description: "Create high-fidelity HTML slides",
    defaultEnabled: false,
  },
  {
    id: "canvas",
    label: "Reports",
    icon: LayoutTemplate,
    description: "Create interactive HTML dashboards",
    defaultEnabled: false,
  },
  {
    id: "research",
    label: "Research",
    icon: Globe,
    description: "Smart web browsing & search",
    defaultEnabled: false,
  },
  /*
  {
    id: "diagrams",
    label: "Diagrams",
    icon: GitBranch,
    description: "Generate flowcharts & diagrams",
    defaultEnabled: false,
  },
  {
    id: "spreadsheets",
    label: "Tables",
    icon: Table,
    description: "Create data tables & CSVs",
    defaultEnabled: false,
  },
  */
];

interface ToolCategoryPickerProps {
  enabledCategories: Set<string>;
  onCategoriesChange: (categories: Set<string>) => void;
  disabled?: boolean;
  // Optional: Auto-enable Plan Mode for certain tools
  onAgentChange?: (agent: string | undefined) => void;
  selectedAgent?: string;
}

export function ToolCategoryPicker({
  enabledCategories,
  onCategoriesChange,
  disabled,
  onAgentChange,
  selectedAgent,
}: ToolCategoryPickerProps) {
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const enabledCount = Array.from(enabledCategories).filter(
    (c) => !["files", "search", "terminal"].includes(c)
  ).length;

  const toggleCategory = (id: string) => {
    const next = new Set(enabledCategories);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);

      // Auto-enable Plan Mode for presentation/canvas tools (requires approval workflow)
      if (
        (id === "presentations" || id === "canvas") &&
        onAgentChange &&
        selectedAgent !== "plan"
      ) {
        onAgentChange("plan");
        console.log(`[AutoPlan] Enabled Plan Mode for ${id} workflow`);
      }
    }
    onCategoriesChange(next);
    // Keep dropdown open so user can toggle multiple tools
    // User can click outside or press button again to close
  };

  // Close dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: globalThis.MouseEvent) => {
      const target = event.target;
      if (!dropdownRef.current || !(target instanceof globalThis.Node)) return;
      if (!dropdownRef.current.contains(target)) {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [isOpen]);

  return (
    <div className="relative" ref={dropdownRef}>
      <button
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        disabled={disabled}
        className={cn(
          "flex h-8 items-center gap-1.5 rounded-md px-2 text-xs font-medium transition-colors",
          disabled
            ? "cursor-not-allowed opacity-50"
            : enabledCount > 0
              ? "bg-primary/10 text-primary"
              : "text-text-muted hover:text-text hover:bg-surface",
          isOpen && "bg-surface text-text"
        )}
        title="Toggle specialized tools"
      >
        <Wrench className="h-3.5 w-3.5" />
        {enabledCount > 0 && (
          <span className="bg-primary text-white text-[10px] px-1 rounded">{enabledCount}</span>
        )}
        <span>Tools</span>
        <ChevronDown className={cn("h-3 w-3 transition-transform", isOpen && "rotate-180")} />
      </button>

      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 10 }}
            transition={{ duration: 0.15 }}
            className="absolute left-0 bottom-full z-50 mb-2 w-56 rounded-lg border border-border bg-surface-elevated shadow-lg"
          >
            <div className="p-2 border-b border-border">
              <p className="text-xs font-medium text-text">Specialized Tools</p>
              <p className="text-[10px] text-text-muted">Enable to add capabilities</p>
            </div>
            <div className="p-1">
              {TOOL_CATEGORIES.map((category) => {
                const Icon = category.icon;
                const isEnabled = enabledCategories.has(category.id);

                return (
                  <button
                    key={category.id}
                    type="button"
                    onClick={() => toggleCategory(category.id)}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors",
                      isEnabled ? "bg-primary/10 text-primary" : "text-text hover:bg-surface"
                    )}
                  >
                    <div
                      className={cn(
                        "h-4 w-4 rounded border flex items-center justify-center",
                        isEnabled ? "bg-primary border-primary" : "border-border"
                      )}
                    >
                      {isEnabled && <span className="text-white text-[10px]">✓</span>}
                    </div>
                    <Icon className="h-3.5 w-3.5 flex-shrink-0" />
                    <div className="flex-1 min-w-0">
                      <div className="text-xs font-medium">{category.label}</div>
                      <div className="text-[10px] text-text-muted">{category.description}</div>
                    </div>
                  </button>
                );
              })}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
