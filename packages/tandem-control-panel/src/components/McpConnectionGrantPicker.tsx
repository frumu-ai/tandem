import { useEffect, useMemo, useRef } from "react";
import type { StudioMcpConnectionGrant } from "../features/studio/schema";
import {
  mcpConnectionClassLabel,
  mcpConnectionClassTone,
  mcpConnectionGrantFor,
  mcpConnectionGrantKey,
  mcpConnectionOwnerLabel,
  mcpConnectionScopeLabel,
  mcpConnectionStatusLabel,
  normalizeMcpConnectionGrants,
  type McpConnectionSummary,
} from "../features/mcp/mcpConnections";
import { Icon } from "../ui/Icon";

type McpConnectionGrantPickerProps = {
  title: string;
  subtitle?: string;
  connections: McpConnectionSummary[];
  value: StudioMcpConnectionGrant[];
  onChange: (next: StudioMcpConnectionGrant[]) => void;
  selectedServers?: string[];
  onSelectedServersChange?: (next: string[]) => void;
  emptyText?: string;
};

function uniqueStrings(values: string[]) {
  return Array.from(new Set(values.map((value) => String(value || "").trim()).filter(Boolean)));
}

export function McpConnectionGrantPicker({
  title,
  subtitle,
  connections,
  value,
  onChange,
  selectedServers = [],
  onSelectedServersChange,
  emptyText = "No scoped MCP connections are visible for the selected tenant.",
}: McpConnectionGrantPickerProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const grants = useMemo(() => normalizeMcpConnectionGrants(value), [value]);
  const selectedGrantKeys = useMemo(
    () => new Set(grants.map((grant) => mcpConnectionGrantKey(grant))),
    [grants]
  );
  const visibleConnectionKeys = useMemo(
    () =>
      new Set(
        connections.map((connection) => mcpConnectionGrantKey(mcpConnectionGrantFor(connection)))
      ),
    [connections]
  );
  const missingGrants = useMemo(
    () => grants.filter((grant) => !visibleConnectionKeys.has(mcpConnectionGrantKey(grant))),
    [grants, visibleConnectionKeys]
  );

  const setGrants = (next: StudioMcpConnectionGrant[]) => {
    onChange(normalizeMcpConnectionGrants(next));
  };

  const toggleConnection = (connection: McpConnectionSummary) => {
    const grant = mcpConnectionGrantFor(connection);
    const key = mcpConnectionGrantKey(grant);
    if (selectedGrantKeys.has(key)) {
      setGrants(grants.filter((entry) => mcpConnectionGrantKey(entry) !== key));
      return;
    }
    setGrants([...grants, grant]);
    if (onSelectedServersChange && !selectedServers.includes(connection.server)) {
      onSelectedServersChange(uniqueStrings([...selectedServers, connection.server]).sort());
    }
  };

  const removeGrant = (grant: StudioMcpConnectionGrant) => {
    const key = mcpConnectionGrantKey(grant);
    setGrants(grants.filter((entry) => mcpConnectionGrantKey(entry) !== key));
  };

  return (
    <div
      ref={rootRef}
      className="grid gap-3 rounded-xl border border-slate-700/70 bg-slate-950/30 p-3"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-medium text-slate-100">
            <Icon name="key-round" className="h-4 w-4" />
            <span>{title}</span>
          </div>
          {subtitle ? <div className="mt-1 text-xs text-slate-400">{subtitle}</div> : null}
        </div>
        <span className="tcp-badge-info">{grants.length} selected</span>
      </div>

      {connections.length ? (
        <div className="grid gap-2">
          {connections.map((connection) => {
            const grant = mcpConnectionGrantFor(connection);
            const selected = selectedGrantKeys.has(mcpConnectionGrantKey(grant));
            return (
              <label
                key={connection.connectionId}
                className={`grid cursor-pointer gap-2 rounded-lg border px-3 py-2 text-sm transition ${
                  selected
                    ? "border-amber-400/50 bg-amber-400/10"
                    : "border-slate-700/60 bg-slate-950/20"
                }`}
              >
                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="flex min-w-0 items-start gap-2">
                    <input
                      type="checkbox"
                      className="mt-1"
                      checked={selected}
                      onChange={() => toggleConnection(connection)}
                    />
                    <div className="min-w-0">
                      <div className="break-words font-medium text-slate-100">
                        {connection.server} · {mcpConnectionOwnerLabel(connection)}
                      </div>
                      <div className="mt-0.5 break-words tcp-text-caption text-slate-500">
                        {connection.connectionId}
                      </div>
                    </div>
                  </div>
                  <div className="flex flex-wrap justify-end gap-1">
                    <span className={mcpConnectionClassTone(connection)}>
                      {mcpConnectionClassLabel(connection)}
                    </span>
                    <span className={connection.connected ? "tcp-badge-ok" : "tcp-badge-warn"}>
                      {mcpConnectionStatusLabel(connection)}
                    </span>
                  </div>
                </div>
                <div className="flex flex-wrap gap-2 tcp-text-caption text-slate-400">
                  <span>{mcpConnectionScopeLabel(connection)}</span>
                  <span>Tools: {connection.toolCount}</span>
                </div>
              </label>
            );
          })}
        </div>
      ) : (
        <div className="text-xs text-slate-500">{emptyText}</div>
      )}

      {missingGrants.length ? (
        <div className="grid gap-2 border-t border-slate-800 pt-2">
          <div className="tcp-text-caption uppercase tracking-wide text-amber-200">
            Revoked or invalid connector grants
          </div>
          {missingGrants.map((grant) => (
            <div
              key={mcpConnectionGrantKey(grant)}
              className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-100"
            >
              <span className="break-all">
                {grant.server}
                {grant.connection_id ? ` · ${grant.connection_id}` : ""}
              </span>
              <button
                type="button"
                className="tcp-btn h-7 px-2 text-xs"
                onClick={() => removeGrant(grant)}
              >
                <Icon name="x" />
                Remove
              </button>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
