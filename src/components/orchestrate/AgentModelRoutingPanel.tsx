import { ModelSelector } from "@/components/chat/ModelSelector";
import { Button } from "@/components/ui";
import type { OrchestratorModelRouting, OrchestratorModelSelection } from "./types";

interface AgentModelRoutingPanelProps {
  routing: OrchestratorModelRouting;
  onChange: (next: OrchestratorModelRouting) => void;
  disabled?: boolean;
  className?: string;
}

const ROLE_LABELS: Record<keyof OrchestratorModelRouting, string> = {
  planner: "Planner",
  builder: "Builder",
  validator: "Validator",
};

function selectionLabel(selection?: OrchestratorModelSelection | null): string {
  if (!selection?.model || !selection?.provider) return "Use run default";
  return `${selection.provider} / ${selection.model}`;
}

export function AgentModelRoutingPanel({
  routing,
  onChange,
  disabled = false,
  className,
}: AgentModelRoutingPanelProps) {
  const setRoleSelection = (
    role: keyof OrchestratorModelRouting,
    selection: OrchestratorModelSelection | null
  ) => {
    onChange({
      ...routing,
      [role]: selection,
    });
  };

  return (
    <div
      className={`rounded-lg border border-border bg-surface-elevated/40 p-3 ${className ?? ""}`}
    >
      <div className="mb-2 text-[10px] uppercase tracking-wide text-text-subtle">Agent Models</div>
      <div className="space-y-3">
        {(Object.keys(ROLE_LABELS) as Array<keyof OrchestratorModelRouting>).map((role) => {
          const selection = routing[role];
          return (
            <div key={role} className="rounded border border-border/70 bg-surface px-3 py-2">
              <div className="mb-2 flex items-center justify-between gap-2">
                <div>
                  <div className="text-xs font-medium text-text">{ROLE_LABELS[role]}</div>
                  <div className="text-[11px] text-text-subtle">{selectionLabel(selection)}</div>
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={disabled || !selection}
                  onClick={() => setRoleSelection(role, null)}
                >
                  Use Default
                </Button>
              </div>
              <ModelSelector
                currentModel={selection?.model ?? undefined}
                align="left"
                side="bottom"
                onModelSelect={(modelId, providerId) =>
                  setRoleSelection(role, { model: modelId, provider: providerId })
                }
                className={disabled ? "pointer-events-none opacity-70" : undefined}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
