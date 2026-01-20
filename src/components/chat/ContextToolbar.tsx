import { AgentSelector } from "./AgentSelector";
import { ToolCategoryPicker } from "./ToolCategoryPicker";
import { ModelSelector } from "./ModelSelector";
import type { ModelInfo } from "@/lib/tauri";
import { ShieldCheck, ShieldOff } from "lucide-react";

interface ContextToolbarProps {
  // Agent
  selectedAgent?: string;
  onAgentChange?: (agent: string | undefined) => void;
  // Tools
  enabledToolCategories: Set<string>;
  onToolCategoriesChange: (categories: Set<string>) => void;
  // Model (optional for future use)
  selectedModel?: string;
  onModelChange?: (model: string) => void;
  availableModels?: ModelInfo[];
  // Provider indicator
  activeProviderLabel?: string;
  activeModelLabel?: string;
  allowAllTools?: boolean;
  onAllowAllToolsChange?: (allow: boolean) => void;
  allowAllToolsLocked?: boolean;
  // State
  disabled?: boolean;
}

export function ContextToolbar({
  selectedAgent,
  onAgentChange,
  enabledToolCategories,
  onToolCategoriesChange,
  selectedModel,
  onModelChange,
  availableModels,
  activeProviderLabel,
  activeModelLabel,
  allowAllTools,
  onAllowAllToolsChange,
  allowAllToolsLocked,
  disabled,
}: ContextToolbarProps) {
  const providerSummary = activeProviderLabel
    ? `${activeProviderLabel}${activeModelLabel ? ` · ${activeModelLabel}` : ""}`
    : null;

  return (
    <div className="flex items-center gap-2 px-3 py-2 border-t border-border/50 bg-surface/30">
      {/* Agent selector */}
      {onAgentChange && (
        <AgentSelector
          selectedAgent={selectedAgent}
          onAgentChange={onAgentChange}
          disabled={disabled}
        />
      )}

      {/* Divider */}
      {onAgentChange && <div className="h-4 w-px bg-border" />}

      {/* Tool categories */}
      <ToolCategoryPicker
        enabledCategories={enabledToolCategories}
        onCategoriesChange={onToolCategoriesChange}
        onAgentChange={onAgentChange}
        selectedAgent={selectedAgent}
        disabled={disabled}
      />

      {onAllowAllToolsChange && (
        <>
          <div className="h-4 w-px bg-border" />
          <button
            type="button"
            onClick={() => onAllowAllToolsChange(!allowAllTools)}
            disabled={disabled || allowAllToolsLocked}
            className={`flex items-center gap-1 rounded-md border px-2 py-1 text-[10px] transition-colors ${
              allowAllTools
                ? "border-emerald-500/40 bg-emerald-500/15 text-emerald-200"
                : "border-border bg-surface-elevated text-text-subtle hover:text-text"
            } ${allowAllToolsLocked ? "cursor-not-allowed opacity-60" : ""}`}
            title={
              allowAllToolsLocked
                ? "Allow all was applied when this chat was created."
                : "Skip tool permission prompts for this new chat."
            }
          >
            {allowAllTools ? (
              <ShieldCheck className="h-3 w-3" />
            ) : (
              <ShieldOff className="h-3 w-3" />
            )}
            <span>Allow all</span>
          </button>
        </>
      )}

      {providerSummary && (
        <>
          <div className="h-4 w-px bg-border" />
          <div
            className="max-w-[200px] truncate rounded-md bg-surface-elevated px-2 py-1 text-[10px] text-text-muted"
            title={providerSummary}
          >
            <span className="font-medium text-text">{activeProviderLabel}</span>
            {activeModelLabel && <span className="text-text-subtle"> · {activeModelLabel}</span>}
          </div>
        </>
      )}

      {/* Model selector (optional) */}
      {onModelChange && availableModels && availableModels.length > 0 && (
        <>
          {/* Divider */}
          <div className="h-4 w-px bg-border" />

          <ModelSelector
            selectedModel={selectedModel}
            onModelChange={onModelChange}
            models={availableModels}
            disabled={disabled}
          />
        </>
      )}

      {/* Spacer to push hints right */}
      <div className="flex-1" />

      {/* Compact hints */}
      <span className="text-[10px] text-text-subtle hidden sm:inline">
        Enter to send • Shift+Enter for newline
      </span>
    </div>
  );
}
