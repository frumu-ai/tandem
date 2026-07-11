import { useEffect, useRef } from "react";
import { EmptyState, SearchInput } from "../ui/index.tsx";
import { Icon } from "../ui/Icon";

export function WorkspaceDirectoryPicker({
  value,
  error = "",
  open,
  browseDir,
  search,
  parentDir,
  currentDir,
  directories,
  label = "Workspace root",
  helperText = "Select the local workspace folder the planner should inspect.",
  onOpen,
  onClose,
  onClear,
  onSearchChange,
  onBrowseParent,
  onBrowseDirectory,
  onSelectDirectory,
}: {
  value: string;
  error?: string;
  open: boolean;
  browseDir: string;
  search: string;
  parentDir: string;
  currentDir: string;
  directories: any[];
  label?: string;
  helperText?: string;
  onOpen: () => void;
  onClose: () => void;
  onClear: () => void;
  onSearchChange: (value: string) => void;
  onBrowseParent: () => void;
  onBrowseDirectory: (path: string) => void;
  onSelectDirectory: () => void;
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const searchQuery = String(search || "")
    .trim()
    .toLowerCase();

  return (
    <div ref={rootRef} className="contents">
      <label className="grid gap-2">
        <span className="text-xs uppercase tracking-wide text-slate-500">{label}</span>
        <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
          <button className="tcp-btn h-10 px-3" type="button" onClick={onOpen}>
            <Icon name="folder-open" />
            Browse
          </button>
          <input
            className={`tcp-input text-sm ${error ? "border-red-500/70 text-red-100" : ""}`}
            value={value}
            readOnly
            placeholder="Select a local directory with Browse"
          />
          <button className="tcp-btn h-10 px-3" type="button" onClick={onClear} disabled={!value}>
            Clear
          </button>
        </div>
        <span className={`text-xs ${error ? "text-red-300" : "tcp-subtle"}`}>
          {error || helperText}
        </span>
      </label>

      {open ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
          <button
            type="button"
            className="tcp-confirm-backdrop"
            aria-label="Close workspace directory dialog"
            onClick={onClose}
          />
          <div className="tcp-confirm-dialog max-w-2xl">
            <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
            <p className="tcp-confirm-message">Current: {currentDir || browseDir || "n/a"}</p>
            <div className="mb-2 flex flex-wrap gap-2">
              <button
                className="tcp-btn"
                type="button"
                onClick={onBrowseParent}
                disabled={!parentDir}
              >
                <Icon name="arrow-up" />
                Up
              </button>
              <button
                className="tcp-btn-primary"
                type="button"
                onClick={onSelectDirectory}
                disabled={!currentDir}
              >
                Select This Folder
              </button>
              <button className="tcp-btn" type="button" onClick={onClose}>
                Close
              </button>
            </div>
            <div className="mb-2">
              <SearchInput
                className="tcp-input"
                aria-label="Filter folders"
                placeholder="Type to filter folders..."
                value={search}
                onInput={(event) => onSearchChange((event.target as HTMLInputElement).value)}
              />
            </div>
            <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
              {directories.length ? (
                directories.map((entry: any) => (
                  <button
                    key={String(entry?.path || entry?.name)}
                    className="tcp-list-item mb-1 w-full text-left"
                    type="button"
                    onClick={() => onBrowseDirectory(String(entry?.path || ""))}
                  >
                    <Icon name="folder-open" />
                    <span>{String(entry?.name || entry?.path || "")}</span>
                  </button>
                ))
              ) : (
                <EmptyState
                  text={
                    searchQuery
                      ? "No folders match your search."
                      : "No subdirectories in this folder."
                  }
                />
              )}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
