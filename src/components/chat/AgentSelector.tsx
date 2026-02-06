import { useState, useRef, useEffect } from "react";
import {
  ChevronDown,
  Zap,
  ListChecks,
  Code,
  MessageCircleQuestion,
  Search,
  Sparkles,
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";

export interface Agent {
  id: string | undefined;
  label: string;
  icon: typeof Zap;
  description: string;
}

const AGENTS: Agent[] = [
  { id: undefined, label: "Immediate", icon: Zap, description: "Execute changes directly" },
  { id: "plan", label: "Plan", icon: ListChecks, description: "Propose changes for review" },
  {
    id: "orchestrate",
    label: "Orchestrate",
    icon: Sparkles,
    description: "AI plans & executes multi-step tasks",
  },
  { id: "coder", label: "Coder", icon: Code, description: "Focus on code generation" },
  {
    id: "general",
    label: "Ask",
    icon: MessageCircleQuestion,
    description: "Q&A without making changes",
  },
  { id: "explore", label: "Explore", icon: Search, description: "Analyze and explore code" },
];

interface AgentSelectorProps {
  selectedAgent?: string;
  onAgentChange: (agent: string | undefined) => void;
  disabled?: boolean;
}

export function AgentSelector({ selectedAgent, onAgentChange, disabled }: AgentSelectorProps) {
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const currentAgent = AGENTS.find((a) => a.id === selectedAgent) || AGENTS[0];
  const CurrentIcon = currentAgent.icon;

  // Close dropdown when clicking outside
  useEffect(() => {
    // eslint-disable-next-line no-undef
    const handleClickOutside = (event: Event) => {
      // eslint-disable-next-line no-undef
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", handleClickOutside);
      return () => document.removeEventListener("mousedown", handleClickOutside);
    }
  }, [isOpen]);

  const handleSelect = (agent: Agent) => {
    onAgentChange(agent.id);
    setIsOpen(false);
  };

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
            : "hover:bg-surface text-text-muted hover:text-text",
          isOpen && "bg-surface text-text"
        )}
        title={currentAgent.description}
      >
        <CurrentIcon className="h-3.5 w-3.5" />
        <span>{currentAgent.label}</span>
        <ChevronDown className={cn("h-3 w-3 transition-transform", isOpen && "rotate-180")} />
      </button>

      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 10 }}
            transition={{ duration: 0.15 }}
            className="absolute left-0 bottom-full z-50 mb-2 w-48 rounded-lg border border-border bg-surface-elevated shadow-lg"
          >
            <div className="p-1">
              {AGENTS.map((agent) => {
                const AgentIcon = agent.icon;
                const isSelected = agent.id === selectedAgent;

                return (
                  <button
                    key={agent.id || "immediate"}
                    type="button"
                    onClick={() => handleSelect(agent)}
                    className={cn(
                      "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors",
                      isSelected
                        ? "bg-primary/10 text-primary"
                        : "text-text hover:bg-surface hover:text-text"
                    )}
                  >
                    <AgentIcon className="h-3.5 w-3.5 flex-shrink-0" />
                    <div className="flex-1 min-w-0">
                      <div className="text-xs font-medium">{agent.label}</div>
                      <div className="text-[10px] text-text-muted leading-tight">
                        {agent.description}
                      </div>
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
