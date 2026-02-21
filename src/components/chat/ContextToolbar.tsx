import { AgentSelector } from "./AgentSelector";
import { ToolCategoryPicker } from "./ToolCategoryPicker";
import { ModelSelector } from "./ModelSelector";
import { LoopToggle, LoopStatusChip } from "@/components/ralph";
import type { RalphStateSnapshot } from "@/components/ralph";
import { ScrollText, ShieldCheck, ShieldOff } from "lucide-react";

interface ContextToolbarProps {
  // Agent
  selectedAgent?: string;
  onAgentChange?: (agent: string | undefined) => void;
  // Tools
  enabledToolCategories: Set<string>;
  onToolCategoriesChange: (categories: Set<string>) => void;
  // Provider indicator
  activeProviderLabel?: string;
  activeModelId?: string;
  activeModelLabel?: string;
  allowAllTools?: boolean;
  onAllowAllToolsChange?: (allow: boolean) => void;
  allowAllToolsLocked?: boolean;
  // New prop for model selection
  onModelSelect?: (modelId: string, providerId: string) => void;
  // State
  disabled?: boolean;
  // Ralph Loop
  loopEnabled?: boolean;
  onLoopToggle?: (enabled: boolean) => void;
  loopStatus?: RalphStateSnapshot;
  onLoopPanelOpen?: () => void;
  // Logs viewer
  onLogsOpen?: () => void;
}

export function ContextToolbar({
  selectedAgent,
  onAgentChange,
  enabledToolCategories,
  onToolCategoriesChange,
  activeProviderLabel,
  activeModelId,
  activeModelLabel,
  allowAllTools,
  onAllowAllToolsChange,
  allowAllToolsLocked,
  onModelSelect,
  disabled,
  loopEnabled,
  onLoopToggle,
  loopStatus,
  onLoopPanelOpen,
  onLogsOpen,
}: ContextToolbarProps) {
  // We can remove the old providerSummary logic or keep it as fallback?
  // Actually, we'll replace the static display with ModelSelector entirely if onModelSelect is present.

  // existing logic...
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
              allowAllTools
                ? "Click to disable auto-approval for tools."
                : "Click to auto-approve all tool requests."
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

      {onModelSelect && (
        <>
          <div className="h-4 w-px bg-border" />
          <ModelSelector
            currentModel={activeModelId || activeModelLabel}
            onModelSelect={onModelSelect}
            className="min-w-0 flex-shrink-0"
          />
        </>
      )}

      {/* Fallback for when no onModelSelect but we have labels (fetched from sidecar maybe?) */}
      {!onModelSelect && providerSummary && (
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

      {/* Ralph Loop Toggle */}
      {onLoopToggle && (
        <>
          <div className="h-4 w-px bg-border" />
          <LoopToggle enabled={loopEnabled ?? false} onToggle={onLoopToggle} disabled={disabled} />
        </>
      )}

      {/* Ralph Loop Status Chip */}
      {loopStatus &&
        loopStatus.status !== "idle" &&
        loopStatus.status !== "completed" &&
        onLoopPanelOpen && (
          <>
            <div className="h-4 w-px bg-border" />
            <LoopStatusChip
              status={loopStatus.status}
              iteration={loopStatus.iteration}
              onClick={onLoopPanelOpen}
            />
          </>
        )}

      {/* Logs button (on-demand diagnostics) */}
      {onLogsOpen && (
        <>
          <div className="h-4 w-px bg-border" />
          <button
            type="button"
            onClick={onLogsOpen}
            disabled={disabled}
            className="flex items-center gap-1 rounded-md border border-border bg-surface-elevated px-2 py-1 text-[10px] text-text-subtle transition-colors hover:bg-surface hover:text-text disabled:opacity-50"
            title="Open logs (on-demand)"
          >
            <ScrollText className="h-3 w-3" />
            <span>Logs</span>
          </button>
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
