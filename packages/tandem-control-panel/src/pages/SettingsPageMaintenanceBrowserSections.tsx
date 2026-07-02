import { AnimatePresence, motion } from "motion/react";
import { Badge, PanelCard, Toolbar } from "../ui/index.tsx";
import { useSettingsPageController } from "./SettingsPageController";

type SettingsPageControllerState = ReturnType<typeof useSettingsPageController>;

type SettingsPageMaintenanceBrowserSectionsProps = {
  controller: SettingsPageControllerState;
};

export function SettingsPageMaintenanceBrowserSections({
  controller,
}: SettingsPageMaintenanceBrowserSectionsProps) {
  const {
    activeSection,
    browserIssues,
    browserRecommendations,
    browserSmokeResult,
    browserStatus,
    setDiagnosticsOpen,
    hostedManaged,
    installBrowserMutation,
    localEngine,
    smokeTestBrowserMutation,
    systemHealthQuery,
    worktreeCleanupActionRows,
    worktreeCleanupDryRun,
    worktreeCleanupMutation,
    worktreeCleanupPendingMessage,
    worktreeCleanupRepoRoot,
    worktreeCleanupResult,
    setWorktreeCleanupDryRun,
    setWorktreeCleanupRepoRoot,
    setWorktreeCleanupResult,
  } = controller;
  const safeWorktreeCleanupActionRows = Array.isArray(worktreeCleanupActionRows)
    ? worktreeCleanupActionRows
    : [];
  const safeBrowserIssues = Array.isArray(browserIssues) ? browserIssues : [];

  return (
    <>
      {activeSection === "maintenance" ? (
        <PanelCard
          title="Managed worktree cleanup"
          subtitle="Scan repo-local .tandem/worktrees entries, keep live runtime worktrees, and remove stale or orphaned leftovers."
          actions={
            <Toolbar>
              <button
                className="tcp-btn"
                onClick={() => {
                  setWorktreeCleanupResult(null);
                  void systemHealthQuery.refetch();
                }}
              >
                <i data-lucide="refresh-cw"></i>
                Refresh root
              </button>
              <button
                className="tcp-btn"
                onClick={() =>
                  worktreeCleanupMutation.mutate({
                    repoRoot: worktreeCleanupRepoRoot.trim(),
                    dryRun: true,
                  })
                }
                disabled={worktreeCleanupMutation.isPending || !worktreeCleanupRepoRoot.trim()}
              >
                <i data-lucide="search"></i>
                Preview stale worktrees
              </button>
              <button
                className="tcp-btn-primary"
                onClick={() =>
                  worktreeCleanupMutation.mutate({
                    repoRoot: worktreeCleanupRepoRoot.trim(),
                    dryRun: worktreeCleanupDryRun,
                  })
                }
                disabled={worktreeCleanupMutation.isPending || !worktreeCleanupRepoRoot.trim()}
              >
                <i data-lucide="trash-2"></i>
                {worktreeCleanupMutation.isPending
                  ? "Cleaning up..."
                  : worktreeCleanupDryRun
                    ? "Run preview"
                    : "Clean stale worktrees"}
              </button>
            </Toolbar>
          }
        >
          <div className="grid gap-4">
            <label className="grid gap-2">
              <span className="text-sm font-medium">Repository root</span>
              <input
                className="tcp-input"
                value={worktreeCleanupRepoRoot}
                onInput={(event) =>
                  setWorktreeCleanupRepoRoot((event.target as HTMLInputElement).value)
                }
                placeholder="/absolute/path/to/repo"
              />
            </label>
            <label className="flex items-center gap-3 text-sm">
              <input
                type="checkbox"
                checked={worktreeCleanupDryRun}
                onChange={(event) => setWorktreeCleanupDryRun(event.target.checked)}
              />
              Use dry run when clicking the primary action
            </label>
            <div className="grid gap-3 md:grid-cols-3">
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Detected workspace root</div>
                <div className="mt-1 break-all text-xs">
                  {String(systemHealthQuery.data?.workspace_root || "Unavailable")}
                </div>
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Cleanup target</div>
                <div className="mt-1 break-all text-xs">
                  {worktreeCleanupResult?.managed_root ||
                    `${worktreeCleanupRepoRoot || "repo"}/.tandem/worktrees`}
                </div>
              </div>
              <div className="tcp-list-item">
                <div className="text-sm font-medium">Host mode</div>
                <div className="mt-1 text-xs">
                  {localEngine ? "Local engine" : "Remote engine"} ·{" "}
                  {hostedManaged ? "hosted-managed" : "self-managed"}
                </div>
              </div>
            </div>
            {worktreeCleanupMutation.isPending ? (
              <motion.div
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                className="rounded-2xl border border-cyan-500/30 bg-cyan-500/10 px-4 py-3"
              >
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="text-sm font-medium">Cleanup running</div>
                    <div className="tcp-subtle mt-1 text-xs">{worktreeCleanupPendingMessage}</div>
                  </div>
                  <motion.div
                    className="h-2 w-24 overflow-hidden rounded-full bg-slate-800"
                    initial={false}
                  >
                    <motion.div
                      className="h-full rounded-full bg-cyan-400"
                      animate={{ x: ["-100%", "120%"] }}
                      transition={{ duration: 1.2, repeat: Infinity, ease: "easeInOut" }}
                    />
                  </motion.div>
                </div>
              </motion.div>
            ) : null}
            {worktreeCleanupResult ? (
              <div className="grid gap-3">
                <div className="grid gap-3 md:grid-cols-4">
                  <div className="tcp-list-item">
                    <div className="text-sm font-medium">Tracked active</div>
                    <div className="mt-1 text-2xl font-semibold">
                      {worktreeCleanupResult.active_paths?.length || 0}
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="text-sm font-medium">Stale candidates</div>
                    <div className="mt-1 text-2xl font-semibold">
                      {worktreeCleanupResult.stale_paths?.length || 0}
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="text-sm font-medium">Removed</div>
                    <div className="mt-1 text-2xl font-semibold">
                      {(worktreeCleanupResult.cleaned_worktrees?.length || 0) +
                        (worktreeCleanupResult.orphan_dirs_removed?.length || 0)}
                    </div>
                  </div>
                  <div className="tcp-list-item">
                    <div className="text-sm font-medium">Failures</div>
                    <div className="mt-1 text-2xl font-semibold">
                      {worktreeCleanupResult.failures?.length || 0}
                    </div>
                  </div>
                </div>
                <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <div className="font-medium">
                        {worktreeCleanupResult.dry_run ? "Preview results" : "Cleanup log"}
                      </div>
                      <div className="tcp-subtle mt-1 text-xs">
                        {worktreeCleanupResult.repo_root || worktreeCleanupRepoRoot}
                      </div>
                    </div>
                    <Badge
                      tone={
                        (worktreeCleanupResult.failures?.length || 0) > 0
                          ? "warn"
                          : worktreeCleanupResult.dry_run
                            ? "info"
                            : "ok"
                      }
                    >
                      {worktreeCleanupResult.dry_run ? "Dry run" : "Applied"}
                    </Badge>
                  </div>
                  <div className="mt-3 grid gap-2">
                    <AnimatePresence initial={false}>
                      {safeWorktreeCleanupActionRows.map((row, index) => (
                        <motion.div
                          key={`${row.kind}-${row.title}-${index}`}
                          initial={{ opacity: 0, y: 10 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, y: -8 }}
                          transition={{ duration: 0.18, delay: index * 0.03 }}
                          className="tcp-list-item"
                        >
                          <div className="flex items-start justify-between gap-3">
                            <div className="min-w-0">
                              <div className="text-sm font-medium break-all">{row.title}</div>
                              <div className="tcp-subtle mt-1 text-xs">{row.detail}</div>
                            </div>
                            <Badge tone={row.tone}>
                              {row.kind === "orphan_removed"
                                ? "orphan"
                                : row.kind.replaceAll("_", " ")}
                            </Badge>
                          </div>
                        </motion.div>
                      ))}
                    </AnimatePresence>
                    {!safeWorktreeCleanupActionRows.length ? (
                      <div className="tcp-subtle text-xs">
                        No stale managed worktrees were detected for this repository.
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
            ) : null}
            <div className="tcp-subtle text-xs">
              This action only targets repo-local managed worktrees under{" "}
              <code>.tandem/worktrees</code> and skips paths that the current Tandem process still
              tracks as active.
            </div>
          </div>
        </PanelCard>
      ) : null}

      {activeSection === "browser" ? (
        <PanelCard
          title="Browser readiness"
          subtitle="Operational browser status, diagnostics, and recovery actions."
          actions={
            <Toolbar>
              <button className="tcp-btn" onClick={() => void browserStatus.refetch()}>
                <i data-lucide="refresh-cw"></i>
                Refresh browser status
              </button>
              <button
                className="tcp-btn"
                onClick={() => installBrowserMutation.mutate()}
                disabled={installBrowserMutation.isPending}
              >
                <i data-lucide="download"></i>
                {installBrowserMutation.isPending ? "Installing sidecar..." : "Install sidecar"}
              </button>
              <button
                className="tcp-btn"
                onClick={() => smokeTestBrowserMutation.mutate()}
                disabled={smokeTestBrowserMutation.isPending}
              >
                <i data-lucide="globe"></i>
                {smokeTestBrowserMutation.isPending ? "Running smoke test..." : "Run smoke test"}
              </button>
              <button className="tcp-btn" onClick={() => setDiagnosticsOpen(true)}>
                <i data-lucide="activity"></i>
                Diagnostics
              </button>
            </Toolbar>
          }
        >
          <div className="grid gap-2 md:grid-cols-3">
            <div className="tcp-list-item">
              <div className="text-sm font-medium">Status</div>
              <div className="mt-1 text-sm">
                {browserStatus.data
                  ? browserStatus.data.runnable
                    ? "Ready"
                    : browserStatus.data.enabled
                      ? "Blocked"
                      : "Disabled"
                  : "Unknown"}
              </div>
              <div className="tcp-subtle text-xs">
                Headless default: {browserStatus.data?.headless_default ? "yes" : "no"}
              </div>
            </div>
            <div className="tcp-list-item">
              <div className="text-sm font-medium">Sidecar</div>
              <div className="mt-1 break-all text-sm">
                {browserStatus.data?.sidecar?.path || "Not found"}
              </div>
              <div className="tcp-subtle text-xs">
                {browserStatus.data?.sidecar?.version || "No version detected"}
              </div>
            </div>
            <div className="tcp-list-item">
              <div className="text-sm font-medium">Browser</div>
              <div className="mt-1 break-all text-sm">
                {browserStatus.data?.browser?.path || "Not found"}
              </div>
              <div className="tcp-subtle text-xs">
                {browserStatus.data?.browser?.version ||
                  browserStatus.data?.browser?.channel ||
                  "No version detected"}
              </div>
            </div>
          </div>
          {safeBrowserIssues.length ? (
            <div className="mt-3 grid gap-2">
              {safeBrowserIssues.map((issue, index) => (
                <div key={`${issue.code || "issue"}-${index}`} className="tcp-list-item">
                  <div className="text-sm font-medium">{issue.code || "browser_issue"}</div>
                  <div className="tcp-subtle text-xs">
                    {issue.message || "Unknown browser issue."}
                  </div>
                </div>
              ))}
            </div>
          ) : null}
          {browserSmokeResult ? (
            <div className="mt-3 rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
              <div className="font-medium">
                Smoke test passed
                {browserSmokeResult.title ? `: ${browserSmokeResult.title}` : ""}
              </div>
              <div className="tcp-subtle mt-1 text-xs">
                {browserSmokeResult.final_url || browserSmokeResult.url || "No URL returned"}
              </div>
              <div className="tcp-subtle text-xs">
                Load state: {browserSmokeResult.load_state || "unknown"} · elements:{" "}
                {String(browserSmokeResult.element_count ?? 0)} · closed:{" "}
                {browserSmokeResult.closed ? "yes" : "no"}
              </div>
              {browserSmokeResult.excerpt ? (
                <pre className="tcp-code mt-2 max-h-32 overflow-auto whitespace-pre-wrap break-words">
                  {browserSmokeResult.excerpt}
                </pre>
              ) : null}
            </div>
          ) : null}
        </PanelCard>
      ) : null}
    </>
  );
}
