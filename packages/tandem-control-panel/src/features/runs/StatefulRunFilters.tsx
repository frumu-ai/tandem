import {
  DEFAULT_STATEFUL_RUN_FILTERS,
  RUN_SOURCE_FILTERS,
  RUN_STATUS_FILTERS,
  normalizeStatefulRunFilters,
} from "../../../lib/runs/stateful-runs.js";

type StatefulRunFilterBarProps = {
  filters: any;
  onFiltersChange: (filters: any) => void;
};

function setFilter(filters: any, key: string, value: string) {
  return normalizeStatefulRunFilters({ ...filters, [key]: value });
}

export function hasStatefulRunFilters(filters: any) {
  return (
    JSON.stringify(normalizeStatefulRunFilters(filters)) !==
    JSON.stringify(DEFAULT_STATEFUL_RUN_FILTERS)
  );
}

export function StatefulRunFilterBar({ filters, onFiltersChange }: StatefulRunFilterBarProps) {
  const normalized = normalizeStatefulRunFilters(filters);
  const hasFilters = hasStatefulRunFilters(normalized);
  const update = (key: string, value: string) => onFiltersChange(setFilter(normalized, key, value));

  return (
    <div className="grid gap-2 md:grid-cols-2 xl:grid-cols-4 2xl:grid-cols-[1.4fr_repeat(5,minmax(0,1fr))_auto]">
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Search</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.query}
          onChange={(event) => update("query", event.currentTarget.value)}
          placeholder="Run, workflow, trigger"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Status</span>
        <select
          className="tcp-input h-9 text-sm"
          value={normalized.status}
          onChange={(event) => update("status", event.currentTarget.value)}
        >
          {RUN_STATUS_FILTERS.map((option: any) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Source</span>
        <select
          className="tcp-input h-9 text-sm"
          value={normalized.source}
          onChange={(event) => update("source", event.currentTarget.value)}
        >
          {RUN_SOURCE_FILTERS.map((option: any) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Tenant</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.tenant}
          onChange={(event) => update("tenant", event.currentTarget.value)}
          placeholder="Org or workspace"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Workspace</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.workspace}
          onChange={(event) => update("workspace", event.currentTarget.value)}
          placeholder="Path"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Org Unit</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.orgUnit}
          onChange={(event) => update("orgUnit", event.currentTarget.value)}
          placeholder="Unit or owner"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Resource</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.resource}
          onChange={(event) => update("resource", event.currentTarget.value)}
          placeholder="Kind or ID"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Policy</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.policy}
          onChange={(event) => update("policy", event.currentTarget.value)}
          placeholder="Version"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Data</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.dataClass}
          onChange={(event) => update("dataClass", event.currentTarget.value)}
          placeholder="Class"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Knowledge</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.knowledge}
          onChange={(event) => update("knowledge", event.currentTarget.value)}
          placeholder="Source"
        />
      </label>
      <label className="grid min-w-0 gap-1 text-xs text-tcp-text-muted">
        <span>Phase</span>
        <input
          className="tcp-input h-9 text-sm"
          value={normalized.wait}
          onChange={(event) => update("wait", event.currentTarget.value)}
          placeholder="Wait or retry"
        />
      </label>
      <div className="flex items-end">
        <button
          type="button"
          className="tcp-btn h-9 w-full px-3 text-xs"
          onClick={() => onFiltersChange(DEFAULT_STATEFUL_RUN_FILTERS)}
          disabled={!hasFilters}
        >
          <i data-lucide="x"></i>
          Clear
        </button>
      </div>
    </div>
  );
}
