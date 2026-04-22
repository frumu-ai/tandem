import { AnimatePresence, motion } from "motion/react";
import { ProviderModelSelector } from "../../../components/ProviderModelSelector";
import { EmptyState } from "../../../pages/ui";

type ExecutionMode = "single" | "team" | "swarm";
type WorkflowToolAccessMode = "all" | "custom";

type ProviderOption = { id: string; models: string[] };
type McpServerOption = {
  name: string;
  connected: boolean;
  enabled: boolean;
  authKind: string;
  lastError: string;
  pendingAuth: boolean;
  authMessage: string;
  authorizationUrl: string;
};
type ExecutionModeInfo = {
  id: ExecutionMode;
  label: string;
  icon: string;
  desc: string;
  bestFor: string;
};

type Step3ModeProps = {
  selected: ExecutionMode;
  onSelect: (mode: ExecutionMode) => void;
  executionModes: ExecutionModeInfo[];
  maxAgents: string;
  onMaxAgents: (v: string) => void;
  workspaceRoot: string;
  onWorkspaceRootChange: (v: string) => void;
  providerOptions: ProviderOption[];
  providerId: string;
  modelId: string;
  plannerProviderId: string;
  plannerModelId: string;
  onProviderChange: (v: string) => void;
  onModelChange: (v: string) => void;
  onPlannerProviderChange: (v: string) => void;
  onPlannerModelChange: (v: string) => void;
  roleModelsJson: string;
  onRoleModelsChange: (v: string) => void;
  roleModelsError: string;
  toolAccessMode: WorkflowToolAccessMode;
  customToolsText: string;
  onToolAccessModeChange: (mode: WorkflowToolAccessMode) => void;
  onCustomToolsTextChange: (value: string) => void;
  mcpServers: McpServerOption[];
  selectedMcpServers: string[];
  onToggleMcpServer: (name: string) => void;
  onConnectMcpServer: (name: string) => void;
  onCompleteMcpSignIn: (name: string) => void;
  onRefreshMcpServers: () => void;
  onOpenMcpSettings: () => void;
  activeMcpAction: { name: string; action: string } | null;
  workspaceRootError: string;
  plannerModelError: string;
  workspaceBrowserOpen: boolean;
  workspaceBrowserDir: string;
  workspaceBrowserSearch: string;
  onWorkspaceBrowserSearchChange: (value: string) => void;
  onOpenWorkspaceBrowser: () => void;
  onCloseWorkspaceBrowser: () => void;
  onBrowseWorkspaceParent: () => void;
  onBrowseWorkspaceDirectory: (path: string) => void;
  onSelectWorkspaceDirectory: () => void;
  workspaceBrowserParentDir: string;
  workspaceCurrentBrowseDir: string;
  filteredWorkspaceDirectories: any[];
};

export function Step3Mode(props: Step3ModeProps) {
  const {
    selected,
    onSelect,
    executionModes,
    maxAgents,
    onMaxAgents,
    workspaceRoot,
    onWorkspaceRootChange,
    providerOptions,
    providerId,
    modelId,
    plannerProviderId,
    plannerModelId,
    onProviderChange,
    onModelChange,
    onPlannerProviderChange,
    onPlannerModelChange,
    roleModelsJson,
    onRoleModelsChange,
    roleModelsError,
    toolAccessMode,
    customToolsText,
    onToolAccessModeChange,
    onCustomToolsTextChange,
    mcpServers,
    selectedMcpServers,
    onToggleMcpServer,
    onConnectMcpServer,
    onCompleteMcpSignIn,
    onRefreshMcpServers,
    onOpenMcpSettings,
    activeMcpAction,
    workspaceRootError,
    plannerModelError,
    workspaceBrowserOpen,
    workspaceBrowserDir,
    workspaceBrowserSearch,
    onWorkspaceBrowserSearchChange,
    onOpenWorkspaceBrowser,
    onCloseWorkspaceBrowser,
    onBrowseWorkspaceParent,
    onBrowseWorkspaceDirectory,
    onSelectWorkspaceDirectory,
    workspaceBrowserParentDir,
    workspaceCurrentBrowseDir,
    filteredWorkspaceDirectories,
  } = props;

  const workspaceSearchQuery = String(workspaceBrowserSearch || "")
    .trim()
    .toLowerCase();

  return (
    <div className="grid gap-4">
      <p className="text-sm text-slate-400">
        How should the AI handle this task? (You can always change this later.)
      </p>
      <div className="grid gap-3">
        {executionModes.map((m) => (
          <button
            key={m.id}
            onClick={() => onSelect(m.id)}
            className={`tcp-list-item flex items-start gap-4 text-left transition-all ${
              selected === m.id ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <span className="mt-0.5 text-2xl">{m.icon}</span>
            <div className="grid gap-1">
              <div className="flex items-center gap-2">
                <span className="font-semibold">{m.label}</span>
                {m.id === "team" ? (
                  <span className="rounded-full bg-amber-500/20 px-2 py-0.5 text-xs text-amber-300">
                    Recommended
                  </span>
                ) : null}
              </div>
              <span className="text-sm text-slate-300">{m.desc}</span>
              <span className="tcp-subtle text-xs">Best for: {m.bestFor}</span>
            </div>
            <div
              className="ml-auto mt-1 h-4 w-4 shrink-0 rounded-full border-2 border-slate-600 transition-all data-[checked]:border-amber-400 data-[checked]:bg-amber-400/30"
              data-checked={selected === m.id ? true : undefined}
            />
          </button>
        ))}
      </div>
      {selected !== "single" ? (
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Max parallel agents</label>
          <input
            type="number"
            min="2"
            max="16"
            className="tcp-input w-24"
            value={maxAgents}
            onInput={(e) => onMaxAgents((e.target as HTMLInputElement).value)}
          />
          <div className="text-xs text-slate-500">
            {selected === "team"
              ? "Team mode uses this as the concurrency cap for a small set of specialized agents."
              : "Swarm mode uses this as the concurrency cap when Tandem fans work out in parallel."}
          </div>
        </div>
      ) : null}
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Execution Directory</div>
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Workspace root</label>
          <div className="grid gap-2 md:grid-cols-[auto_1fr_auto]">
            <button className="tcp-btn" type="button" onClick={onOpenWorkspaceBrowser}>
              Browse
            </button>
            <input
              className={`tcp-input text-sm ${workspaceRootError ? "border-red-500/70 text-red-100" : ""}`}
              value={workspaceRoot}
              readOnly
              placeholder="No local directory selected. Use Browse."
            />
            <button
              className="tcp-btn"
              type="button"
              onClick={() => onWorkspaceRootChange("")}
              disabled={!workspaceRoot}
            >
              Clear
            </button>
          </div>
          <div className="text-xs text-slate-500">
            Tandem will run this automation from this workspace directory.
          </div>
          {workspaceRootError ? (
            <div className="text-xs text-red-300">{workspaceRootError}</div>
          ) : null}
        </div>
      </div>
      <AnimatePresence>
        {workspaceBrowserOpen ? (
          <motion.div
            className="fixed inset-0 z-50 flex items-center justify-center p-4"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <button
              type="button"
              className="tcp-confirm-backdrop"
              aria-label="Close workspace directory dialog"
              onClick={onCloseWorkspaceBrowser}
            />
            <motion.div
              className="tcp-confirm-dialog max-w-2xl"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
            >
              <h3 className="tcp-confirm-title">Select Workspace Folder</h3>
              <p className="tcp-confirm-message">Current: {workspaceCurrentBrowseDir || "n/a"}</p>
              <div className="mb-2 flex flex-wrap gap-2">
                <button
                  className="tcp-btn"
                  type="button"
                  onClick={onBrowseWorkspaceParent}
                  disabled={!workspaceBrowserParentDir}
                >
                  Up
                </button>
                <button
                  className="tcp-btn-primary"
                  type="button"
                  onClick={onSelectWorkspaceDirectory}
                  disabled={!workspaceCurrentBrowseDir}
                >
                  Select This Folder
                </button>
                <button className="tcp-btn" type="button" onClick={onCloseWorkspaceBrowser}>
                  Close
                </button>
              </div>
              <div className="mb-2">
                <input
                  className="tcp-input"
                  placeholder="Type to filter folders..."
                  value={workspaceBrowserSearch}
                  onInput={(e) =>
                    onWorkspaceBrowserSearchChange((e.target as HTMLInputElement).value)
                  }
                />
              </div>
              <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
                {filteredWorkspaceDirectories.length ? (
                  filteredWorkspaceDirectories.map((entry: any) => (
                    <button
                      key={String(entry?.path || entry?.name)}
                      className="tcp-list-item mb-1 w-full text-left"
                      type="button"
                      onClick={() => onBrowseWorkspaceDirectory(String(entry?.path || ""))}
                    >
                      {String(entry?.name || entry?.path || "")}
                    </button>
                  ))
                ) : (
                  <EmptyState
                    text={
                      workspaceSearchQuery
                        ? "No folders match your search."
                        : "No subdirectories in this folder."
                    }
                  />
                )}
              </div>
            </motion.div>
          </motion.div>
        ) : null}
      </AnimatePresence>
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Model Selection</div>
        <ProviderModelSelector
          providerLabel="Provider"
          modelLabel="Model"
          draft={{ provider: providerId, model: modelId }}
          providers={providerOptions}
          onChange={(draft) => {
            onProviderChange(draft.provider);
            onModelChange(draft.model);
          }}
          inheritLabel="Use workspace default"
        />
        <div className="grid gap-2 rounded-lg border border-slate-800/70 bg-slate-950/30 p-3">
          <div className="text-xs uppercase tracking-wide text-slate-500">
            Planner fallback model
          </div>
          <div className="text-xs text-slate-400">
            Optional. Leave blank to use the workflow default model for planning and revisions.
          </div>
          <ProviderModelSelector
            providerLabel="Planner provider"
            modelLabel="Planner model"
            draft={{ provider: plannerProviderId, model: plannerModelId }}
            providers={providerOptions}
            onChange={(draft) => {
              onPlannerProviderChange(draft.provider);
              onPlannerModelChange(draft.model);
            }}
            inheritLabel="Disabled"
          />
          {plannerModelError ? (
            <div className="text-xs text-red-300">{plannerModelError}</div>
          ) : null}
        </div>
        <div className="grid gap-1">
          <label className="text-xs text-slate-400">Role model overrides (advanced JSON)</label>
          <textarea
            className={`tcp-input min-h-[72px] font-mono text-xs ${roleModelsError ? "border-red-500/70 text-red-100" : ""}`}
            value={roleModelsJson}
            onInput={(e) => onRoleModelsChange((e.target as HTMLTextAreaElement).value)}
            placeholder={`{\"planner\":{\"provider_id\":\"openai\",\"model_id\":\"gpt-5\"},\"worker\":{\"provider_id\":\"anthropic\",\"model_id\":\"claude-sonnet-4\"}}`}
          />
          {roleModelsError ? <div className="text-xs text-red-300">{roleModelsError}</div> : null}
        </div>
      </div>
      <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="text-xs uppercase tracking-wide text-slate-500">Tool Access</div>
        <div className="grid gap-2 sm:grid-cols-2">
          <button
            type="button"
            className={`tcp-list-item text-left ${toolAccessMode === "all" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
            onClick={() => onToolAccessModeChange("all")}
          >
            <div className="font-medium">All tools</div>
            <div className="tcp-subtle text-xs">Grant full built-in tool access.</div>
          </button>
          <button
            type="button"
            className={`tcp-list-item text-left ${toolAccessMode === "custom" ? "border-amber-400/60 bg-amber-400/10" : ""}`}
            onClick={() => onToolAccessModeChange("custom")}
          >
            <div className="font-medium">Custom allowlist</div>
            <div className="tcp-subtle text-xs">Restrict built-in tools manually.</div>
          </button>
        </div>
        {toolAccessMode === "custom" ? (
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Allowed built-in tools</label>
            <textarea
              className="tcp-input min-h-[96px] font-mono text-xs"
              value={customToolsText}
              onInput={(e) => onCustomToolsTextChange((e.target as HTMLTextAreaElement).value)}
              placeholder={`read\nwrite\nedit\nbash\nls\nglob\nwebsearch`}
            />
          </div>
        ) : (
          <div className="text-xs text-slate-500">All built-in tools are allowed.</div>
        )}
      </div>
      <div className="grid gap-2 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
        <div className="flex items-center justify-between gap-2">
          <div className="text-xs uppercase tracking-wide text-slate-500">MCP Servers</div>
          <div className="flex flex-wrap gap-2">
            <button
              className="tcp-btn h-7 px-2 text-xs"
              type="button"
              onClick={onRefreshMcpServers}
            >
              Reload
            </button>
            <button className="tcp-btn h-7 px-2 text-xs" type="button" onClick={onOpenMcpSettings}>
              Open MCP Settings
            </button>
          </div>
        </div>
        <div className="text-xs text-slate-500">
          Select the servers this automation may use. You can connect existing servers here, or open
          MCP Settings to add/edit servers without losing this draft.
        </div>
        {mcpServers.length ? (
          <div className="grid gap-2">
            {mcpServers.map((server) => {
              const isSelected = selectedMcpServers.includes(server.name);
              const actionPendingForServer = activeMcpAction?.name === server.name;
              const connectPending =
                actionPendingForServer && activeMcpAction?.action === "connect";
              const authPending =
                actionPendingForServer && activeMcpAction?.action === "authenticate";
              return (
                <div
                  key={server.name}
                  className={`grid gap-2 rounded-lg border px-3 py-3 ${
                    isSelected
                      ? "border-amber-400/50 bg-amber-400/10"
                      : "border-slate-700/60 bg-slate-950/20"
                  }`}
                >
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <button
                      type="button"
                      className={`text-left ${isSelected ? "text-amber-200" : "text-slate-100"}`}
                      onClick={() => onToggleMcpServer(server.name)}
                    >
                      <div className="font-medium">{server.name}</div>
                      <div className="text-xs text-slate-400">
                        {isSelected
                          ? "Selected for this automation"
                          : "Click to allow this automation to use the server"}
                      </div>
                    </button>
                    <div className="flex flex-wrap gap-2 text-xs">
                      <span className={server.connected ? "tcp-badge-ok" : "tcp-badge-warn"}>
                        {server.connected ? "Connected" : "Disconnected"}
                      </span>
                      <span className={server.enabled ? "tcp-badge-info" : "tcp-badge-warn"}>
                        {server.enabled ? "Enabled" : "Disabled"}
                      </span>
                    </div>
                  </div>
                  {server.lastError ? (
                    <div className="rounded-lg border border-rose-700/60 bg-rose-950/20 px-3 py-2 text-xs text-rose-200">
                      {server.lastError}
                    </div>
                  ) : null}
                  {server.pendingAuth ? (
                    <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-100">
                      {server.authMessage ||
                        "Browser sign-in is still required for this MCP server."}
                    </div>
                  ) : null}
                  <div className="flex flex-wrap gap-2">
                    {!server.connected ? (
                      <button
                        type="button"
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() => onConnectMcpServer(server.name)}
                        disabled={actionPendingForServer}
                      >
                        {connectPending ? "Connecting..." : "Connect"}
                      </button>
                    ) : null}
                    {(server.pendingAuth || server.authKind === "oauth") && !server.connected ? (
                      <button
                        type="button"
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() => onCompleteMcpSignIn(server.name)}
                        disabled={actionPendingForServer}
                      >
                        {authPending ? "Checking..." : "Mark Sign-In Complete"}
                      </button>
                    ) : null}
                    {server.authorizationUrl ? (
                      <a
                        className="tcp-btn h-7 px-2 text-xs"
                        href={server.authorizationUrl}
                        target="_blank"
                        rel="noreferrer"
                      >
                        Open Sign-In
                      </a>
                    ) : null}
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="text-xs text-slate-400">No MCP servers configured yet.</div>
        )}
      </div>
    </div>
  );
}
