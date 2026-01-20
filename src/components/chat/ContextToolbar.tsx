import { AgentSelector } from "./AgentSelector";
import { ToolCategoryPicker } from "./ToolCategoryPicker";
import { ModelSelector } from "./ModelSelector";
import type { ModelInfo } from "@/lib/tauri";

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
