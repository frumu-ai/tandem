import { useMemo, useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { ModelSelector } from "@/components/chat/ModelSelector";
import { Button } from "@/components/ui";
import type { OrchestratorModelRouting, OrchestratorModelSelection } from "./types";

interface AgentModelRoutingPanelProps {
  routing: OrchestratorModelRouting;
  onChange: (next: OrchestratorModelRouting) => void;
  disabled?: boolean;
  className?: string;
}

const CORE_ROLES = ["orchestrator", "delegator", "worker", "watcher", "reviewer", "tester"];

const ROLE_LABELS: Record<string, string> = {
  orchestrator: "Orchestrator",
  delegator: "Delegator",
  worker: "Worker",
  watcher: "Watcher",
  reviewer: "Reviewer",
  tester: "Tester",
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
  const [customRole, setCustomRole] = useState("");
  const [isExpanded, setIsExpanded] = useState(false);

  const normalizedRouting = useMemo(() => {
    const next: OrchestratorModelRouting = { ...routing };
    const nested = (next as Record<string, unknown>).roles;
    if (nested && typeof nested === "object" && !Array.isArray(nested)) {
      for (const [role, selection] of Object.entries(
        nested as Record<string, OrchestratorModelSelection | null>
      )) {
        next[role] = selection;
      }
    }
    if (next.planner && !next.orchestrator) next.orchestrator = next.planner;
    if (next.builder && !next.worker) next.worker = next.builder;
    if (next.validator && !next.reviewer) next.reviewer = next.validator;
    delete (next as Record<string, unknown>).roles;
    delete (next as Record<string, unknown>).planner;
    delete (next as Record<string, unknown>).builder;
    delete (next as Record<string, unknown>).validator;
    return next;
  }, [routing]);

  const allRoles = useMemo(
    () => Array.from(new Set([...CORE_ROLES, ...Object.keys(normalizedRouting)])).sort(),
    [normalizedRouting]
  );

  const setRoleSelection = (role: string, selection: OrchestratorModelSelection | null) => {
    onChange({
      ...normalizedRouting,
      [role]: selection,
    });
  };

  const addCustomRole = () => {
    const role = customRole.trim().toLowerCase();
    if (!role) return;
    if (normalizedRouting[role] === undefined) {
      onChange({ ...normalizedRouting, [role]: null });
    }
    setCustomRole("");
  };

  return (
    <div
      className={`rounded-lg border border-border bg-surface-elevated/40 p-3 ${className ?? ""}`}
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <div className="text-[10px] uppercase tracking-wide text-text-subtle">
          Orchestrator Role Models
        </div>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => setIsExpanded((prev) => !prev)}
          disabled={disabled}
        >
          {isExpanded ? (
            <>
              <ChevronUp className="mr-1 h-3.5 w-3.5" />
              Collapse
            </>
          ) : (
            <>
              <ChevronDown className="mr-1 h-3.5 w-3.5" />
              Expand
            </>
          )}
        </Button>
      </div>

      <div className="mb-3 text-[11px] text-text-subtle">
        Legacy aliases map automatically: planner-&gt;orchestrator, builder-&gt;worker,
        validator-&gt;reviewer, researcher-&gt;watcher.
      </div>

      {!isExpanded ? (
        <div className="rounded border border-border/70 bg-surface px-3 py-2 text-xs text-text-muted">
          Collapsed. Uses run default model unless a role override is set.
        </div>
      ) : null}

      <div className={`space-y-3 ${isExpanded ? "" : "hidden"}`}>
        {allRoles.map((role) => {
          const selection = normalizedRouting[role];
          return (
            <div key={role} className="rounded border border-border/70 bg-surface px-3 py-2">
              <div className="mb-2 flex items-center justify-between gap-2">
                <div>
                  <div className="text-xs font-medium text-text">{ROLE_LABELS[role] ?? role}</div>
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

        <div className="rounded border border-dashed border-border/70 bg-surface px-3 py-2">
          <div className="mb-2 text-xs font-medium text-text">Custom Role Key</div>
          <div className="flex gap-2">
            <input
              value={customRole}
              onChange={(e) => setCustomRole(e.target.value)}
              placeholder="e.g. analyst"
              className="w-full rounded border border-border bg-surface p-2 text-xs text-text"
              disabled={disabled}
            />
            <Button size="sm" variant="secondary" onClick={addCustomRole} disabled={disabled}>
              Add
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
