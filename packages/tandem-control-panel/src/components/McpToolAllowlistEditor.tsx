import { useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { collapseMcpAllowedToolsSelection, normalizeMcpToolNames } from "../features/mcp/mcpTools";

type McpToolAllowlistEditorProps = {
  title: string;
  subtitle?: string;
  discoveredTools: string[];
  value: string[] | null;
  onChange: (next: string[] | null) => void;
  disabled?: boolean;
  emptyText?: string;
  collapsible?: boolean;
  defaultCollapsed?: boolean;
};

export function McpToolAllowlistEditor({
  title,
  subtitle,
  discoveredTools,
  value,
  onChange,
  disabled = false,
  emptyText = "No MCP tools have been discovered for this server yet.",
  collapsible = false,
  defaultCollapsed = false,
}: McpToolAllowlistEditorProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const [collapsedState, setCollapsed] = useState(defaultCollapsed);
  const collapsed = collapsible ? collapsedState : false;
  const discovered = useMemo(() => normalizeMcpToolNames(discoveredTools), [discoveredTools]);
  const selected = useMemo(() => {
    if (value === null) return [...discovered];
    return normalizeMcpToolNames(value);
  }, [value, discovered]);
  const selectedSet = useMemo(() => new Set(selected), [selected]);
  const discoveredSet = useMemo(() => new Set(discovered), [discovered]);
  const visibleSelectedCount = discovered.filter((tool) => selectedSet.has(tool)).length;
  const extraSelected = selected.filter((tool) => !discoveredSet.has(tool));
  const allVisibleSelected =
    discovered.length > 0 &&
    visibleSelectedCount === discovered.length &&
    extraSelected.length === 0;

  useEffect(() => {
    if (rootRef.current) renderIcons(rootRef.current);
  }, [collapsed, discovered.length, extraSelected.length, visibleSelectedCount]);

  const setSelection = (next: string[] | null) => {
    if (disabled) return;
    onChange(next);
  };

  const toggleTool = (toolName: string) => {
    if (disabled) return;
    const next = new Set(value === null ? discovered : selected);
    if (next.has(toolName)) {
      next.delete(toolName);
    } else {
      next.add(toolName);
    }
    const nextSelected = Array.from(next);
    const collapsed = collapseMcpAllowedToolsSelection(discovered, nextSelected);
    onChange(collapsed === null ? null : nextSelected);
  };
  const body =
    !discovered.length && !extraSelected.length ? (
      <div className="text-xs text-slate-500">{emptyText}</div>
    ) : (
      <>
        {discovered.length ? (
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {discovered.map((toolName) => {
              const checked = value === null || selectedSet.has(toolName);
              return (
                <label
                  key={toolName}
                  className={`flex items-start gap-2 rounded-lg border px-3 py-2 text-sm transition ${
                    checked
                      ? "border-amber-400/40 bg-amber-400/10 text-amber-100"
                      : "border-slate-700/70 bg-slate-950/20 text-slate-300"
                  } ${disabled ? "opacity-60" : "cursor-pointer"}`}
                >
                  <input
                    type="checkbox"
                    className="mt-1"
                    checked={checked}
                    disabled={disabled}
                    onChange={() => toggleTool(toolName)}
                  />
                  <span className="break-all font-mono tcp-text-caption leading-5">{toolName}</span>
                </label>
              );
            })}
          </div>
        ) : null}

        {extraSelected.length ? (
          <div className="grid gap-2">
            <div className="tcp-text-caption uppercase tracking-wide text-amber-200">
              Saved but not currently discovered
            </div>
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              {extraSelected.map((toolName) => (
                <label
                  key={toolName}
                  className={`flex items-start gap-2 rounded-lg border border-amber-400/30 bg-amber-400/10 px-3 py-2 text-sm text-amber-100 ${
                    disabled ? "opacity-60" : "cursor-pointer"
                  }`}
                >
                  <input
                    type="checkbox"
                    className="mt-1"
                    checked
                    disabled={disabled}
                    onChange={() => toggleTool(toolName)}
                  />
                  <span className="break-all font-mono tcp-text-caption leading-5">{toolName}</span>
                </label>
              ))}
            </div>
          </div>
        ) : null}
      </>
    );

  return (
    <div
      ref={rootRef}
      className="grid rounded-xl border border-slate-700/70 bg-slate-950/30 p-3"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 flex-1 items-start gap-2">
          {collapsible ? (
            <button
              type="button"
              className="mt-0.5 rounded-full border border-slate-700/80 p-1 text-slate-300 transition hover:border-slate-500 hover:text-slate-100"
              aria-label={collapsed ? `Expand ${title}` : `Collapse ${title}`}
              aria-expanded={!collapsed}
              onClick={() => setCollapsed((value) => !value)}
            >
              <i
                data-lucide="chevron-down"
                className={`block h-4 w-4 transition-transform ${collapsed ? "" : "rotate-180"}`}
              />
            </button>
          ) : null}
          <div className="grid gap-1">
            <div className="text-sm font-medium text-slate-100">{title}</div>
            {subtitle ? <div className="text-xs text-slate-400">{subtitle}</div> : null}
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2 tcp-text-caption text-slate-400">
          <span className="rounded-full border border-slate-700 px-2 py-1">
            {value === null || allVisibleSelected
              ? "all discovered"
              : `${visibleSelectedCount}/${discovered.length || 0} selected`}
          </span>
          {!collapsed ? (
            <>
              <button
                type="button"
                className="rounded-full border border-slate-700 px-2 py-1 text-slate-200 transition hover:border-slate-500 disabled:cursor-not-allowed disabled:opacity-50"
                disabled={disabled || !discovered.length}
                onClick={() => setSelection(null)}
              >
                Select all
              </button>
              <button
                type="button"
                className="rounded-full border border-slate-700 px-2 py-1 text-slate-200 transition hover:border-slate-500 disabled:cursor-not-allowed disabled:opacity-50"
                disabled={disabled || (!selected.length && !extraSelected.length)}
                onClick={() => setSelection([])}
              >
                Clear all
              </button>
            </>
          ) : null}
        </div>
      </div>

      <div
        className={`grid transition-[grid-template-rows,opacity] duration-200 ease-out ${
          collapsed ? "grid-rows-[0fr] opacity-0" : "grid-rows-[1fr] opacity-100"
        }`}
        aria-hidden={collapsed}
      >
        <div className={`min-h-0 overflow-hidden ${collapsed ? "invisible" : "visible"}`}>
          <div className="grid gap-3 pt-3">{body}</div>
        </div>
      </div>
    </div>
  );
}
