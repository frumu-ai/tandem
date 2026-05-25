import { useState, useRef, useEffect } from "react";
import {
  ChevronDown,
  Zap,
  ListChecks,
  Code,
  MessageCircleQuestion,
  Search,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { cn } from "@/lib/utils";
import { useModes } from "@/hooks/useModes";
import { getModeIconOptionById } from "@/components/settings/modes/modeIcons";
import type { ModeBase, ModeDefinition } from "@/lib/tauri";

export interface Agent {
  id: string;
  label: string;
  icon: LucideIcon;
  description: string;
}

const BUILTIN_MODES: ModeDefinition[] = [
  { id: "immediate", label: "Immediate", base_mode: "immediate", source: "builtin" },
  { id: "plan", label: "Plan", base_mode: "plan", source: "builtin" },
  { id: "orchestrate", label: "Orchestrate", base_mode: "orchestrate", source: "builtin" },
  { id: "coder", label: "Coder", base_mode: "coder", source: "builtin" },
  { id: "ask", label: "Ask", base_mode: "ask", source: "builtin" },
  { id: "explore", label: "Explore", base_mode: "explore", source: "builtin" },
];

interface AgentSelectorProps {
  selectedAgent?: string;
  onAgentChange: (agent: string | undefined) => void;
  disabled?: boolean;
}

const baseModeIcon: Record<ModeBase, LucideIcon> = {
  immediate: Zap,
  plan: ListChecks,
  orchestrate: Sparkles,
  coder: Code,
  ask: MessageCircleQuestion,
  explore: Search,
};

const baseModeDescription: Record<ModeBase, string> = {
  immediate: "Execute changes directly",
  plan: "Propose changes for review",
  orchestrate: "AI plans & executes multi-step tasks",
  coder: "Focus on code generation",
  ask: "Q&A without making changes",
  explore: "Analyze and explore code",
};

function modeToAgent(mode: ModeDefinition): Agent {
  const customIcon = getModeIconOptionById(mode.icon)?.icon;
  const icon = customIcon ?? baseModeIcon[mode.base_mode] ?? Zap;
  const defaultDesc = baseModeDescription[mode.base_mode] ?? "Custom mode";
  const shortCustomDesc =
    mode.source && mode.source !== "builtin" ? `Custom mode - ${defaultDesc}` : defaultDesc;
  return {
    id: mode.id,
    label: mode.label,
    icon,
    description: shortCustomDesc,
  };
}

export function AgentSelector({ selectedAgent, onAgentChange, disabled }: AgentSelectorProps) {
  const { modes, isLoading } = useModes();
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const modeId = selectedAgent ?? "immediate";
  const availableModes = modes.length > 0 ? modes : BUILTIN_MODES;
  const agents = availableModes.map(modeToAgent);

  const currentAgent = agents.find((a) => a.id === modeId) || agents[0];
  const CurrentIcon = currentAgent.icon;

  useEffect(() => {
    if (isLoading) return;
    if (!modeId) return;
    const exists = agents.some((a) => a.id === modeId);
    if (!exists) {
      onAgentChange(undefined);
    }
  }, [agents, isLoading, modeId, onAgentChange]);

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
    onAgentChange(agent.id === "immediate" ? undefined : agent.id);
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
              {agents.map((agent) => {
                const AgentIcon = agent.icon;
                const isSelected = agent.id === modeId;

                return (
                  <button
                    key={agent.id}
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
                      <div
                        className="text-[10px] text-text-muted leading-tight truncate"
                        title={agent.description}
                      >
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
