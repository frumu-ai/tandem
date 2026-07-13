import type { OrchestrationNodeSpec } from "@frumu/tandem-client";
import { useMemo, useState } from "react";
import { Icon } from "../../ui/Icon";
import { SearchInput } from "../../ui/index.tsx";
import { PaletteItem, automationId, automationName } from "./pagePrimitives";

const nodeSections = [
  {
    label: "Waits",
    nodes: [
      { kind: "wait:timer", label: "Timer", icon: "clock" },
      { kind: "wait:approval", label: "Approval", icon: "shield-check" },
      { kind: "wait:webhook", label: "Webhook", icon: "webhook" },
      {
        kind: "wait:external_condition",
        label: "External condition",
        icon: "radio",
      },
    ],
  },
  {
    label: "Terminals",
    nodes: [
      { kind: "terminal:complete", label: "Complete", icon: "square-check-big" },
      { kind: "terminal:pause", label: "Pause", icon: "pause-circle" },
      { kind: "terminal:fail", label: "Fail", icon: "x-circle" },
    ],
  },
] as const;

function nodeKind(node: OrchestrationNodeSpec) {
  if (node.kind === "workflow") return `workflow:${node.automation_id}`;
  if (node.kind === "wait") return `wait:${node.wait.kind}`;
  return `terminal:${node.outcome}`;
}

export function OrchestrationPalette({
  workflows,
  nodes,
  onAdd,
  readOnly,
}: {
  workflows: any[];
  nodes: OrchestrationNodeSpec[];
  onAdd: (kind: string) => void;
  readOnly: boolean;
}) {
  const [search, setSearch] = useState("");
  const instanceCounts = useMemo(() => {
    const counts = new Map<string, number>();
    nodes.forEach((node) => {
      const kind = nodeKind(node);
      counts.set(kind, (counts.get(kind) || 0) + 1);
    });
    return counts;
  }, [nodes]);
  const needle = search.trim().toLowerCase();
  const matchingWorkflows = workflows.filter((workflow) =>
    `${automationName(workflow)} ${automationId(workflow)}`.toLowerCase().includes(needle)
  );
  const matchingSections = nodeSections
    .map((section) => ({
      ...section,
      nodes: section.nodes.filter((node) => node.label.toLowerCase().includes(needle)),
    }))
    .filter((section) => section.nodes.length > 0);
  const hasMatches = matchingWorkflows.length > 0 || matchingSections.length > 0;

  return (
    <aside className="orch-palette" aria-label="Node library">
      <fieldset disabled={readOnly} className="contents">
        <div className="border-b border-[var(--color-border-subtle)] p-3">
          <div className="mb-2 text-xs font-semibold uppercase text-[var(--color-text-subtle)]">
            Node library
          </div>
          <div className="relative">
            <Icon
              name="search"
              className="absolute left-2.5 top-2.5 text-[var(--color-text-subtle)]"
            />
            <SearchInput
              aria-label="Search nodes"
              className="tcp-input w-full pl-8"
              placeholder="Search nodes"
              value={search}
              onInput={(event) => setSearch(event.currentTarget.value)}
            />
          </div>
        </div>
        <div className="grid gap-1 overflow-y-auto p-3">
          {matchingWorkflows.length > 0 ? (
            <>
              <div className="orch-palette-heading">Workflows</div>
              {matchingWorkflows.map((workflow) => {
                const id = automationId(workflow);
                const kind = `workflow:${id}`;
                return (
                  <PaletteItem
                    key={id}
                    kind={kind}
                    label={automationName(workflow)}
                    icon="workflow"
                    instanceCount={instanceCounts.get(kind)}
                    disabled={readOnly}
                    onAdd={onAdd}
                  />
                );
              })}
            </>
          ) : null}
          {matchingSections.map((section) => (
            <div className="contents" key={section.label}>
              <div className="orch-palette-heading mt-3">{section.label}</div>
              {section.nodes.map((node) => (
                <PaletteItem
                  key={node.kind}
                  kind={node.kind}
                  label={node.label}
                  icon={node.icon}
                  instanceCount={instanceCounts.get(node.kind)}
                  disabled={readOnly}
                  onAdd={onAdd}
                />
              ))}
            </div>
          ))}
          {!hasMatches ? (
            <div className="px-2 py-4 text-xs text-[var(--color-text-subtle)]">
              No matching nodes
            </div>
          ) : null}
        </div>
      </fieldset>
    </aside>
  );
}
