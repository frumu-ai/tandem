import { AnimatePresence, motion } from "motion/react";
import { Badge, DetailDrawer, Toolbar } from "../ui/index.tsx";
import { BugMonitorExternalProjectsPanel } from "../components/BugMonitorExternalProjectsPanel";
import { EmptyState } from "./ui";
import {
  HOSTED_CODER_REPO_ROOT,
  HOSTED_TANDEM_DATA_ROOT,
  hostedWorkspaceDirectoryHint,
  inferMcpCatalogAuthKind,
  inferMcpNameFromTransport,
  isGithubCopilotMcpTransport,
  normalizeMcpName,
  useSettingsPageController,
} from "./SettingsPageController";

type SettingsPageControllerState = ReturnType<typeof useSettingsPageController>;

type SettingsPageOverlaysProps = {
  controller: SettingsPageControllerState;
};

export function SettingsPageOverlays({ controller }: SettingsPageOverlaysProps) {
  const {
    api,
    installBrowserMutation,
    browserInstallHints,
    browserIssues,
    browserRecommendations,
    browserSmokeResult,
    browserStatus,
    bugMonitorCurrentBrowseDir,
    bugMonitorSuggestedWorkspaceRoot,
    bugMonitorWorkspaceBrowserOpen,
    bugMonitorWorkspaceBrowserSearch,
    bugMonitorWorkspaceParentDir,
    bugMonitorWorkspaceSearchQuery,
    configuredMcpServerNames,
    diagnosticsOpen,
    filteredBugMonitorWorkspaceDirectories,
    filteredMcpCatalog,
    githubMcpGuideOpen,
    mcpAuthMode,
    mcpAuthPreviewText,
    mcpCatalog,
    mcpCatalogQuery,
    mcpCatalogSearch,
    mcpConnectAfterAdd,
    mcpCustomHeader,
    mcpEditingName,
    mcpExtraHeaders,
    mcpGithubToolsets,
    mcpIsGithubTransport,
    mcpModalOpen,
    mcpModalTab,
    mcpName,
    mcpOauthGuidanceText,
    mcpOauthStartsAfterSave,
    mcpSaveMutation,
    mcpToken,
    mcpTransport,
    smokeTestBrowserMutation,
    setBugMonitorWorkspaceBrowserDir,
    setBugMonitorWorkspaceBrowserOpen,
    setBugMonitorWorkspaceBrowserSearch,
    setBugMonitorWorkspaceRoot,
    setGithubMcpGuideOpen,
    setMcpAuthMode,
    setMcpCatalogSearch,
    setMcpConnectAfterAdd,
    setMcpCustomHeader,
    setMcpExtraHeaders,
    setMcpGithubToolsets,
    setMcpModalOpen,
    setMcpModalTab,
    setMcpName,
    setMcpToken,
    setDiagnosticsOpen,
    setMcpTransport,
    toast,
  } = controller;
  const safeFilteredBugMonitorWorkspaceDirectories = Array.isArray(
    filteredBugMonitorWorkspaceDirectories
  )
    ? filteredBugMonitorWorkspaceDirectories
    : [];
  const safeBrowserIssues = Array.isArray(browserIssues) ? browserIssues : [];
  const safeBrowserRecommendations = Array.isArray(browserRecommendations)
    ? browserRecommendations
    : [];
  const safeBrowserInstallHints = Array.isArray(browserInstallHints) ? browserInstallHints : [];
  const safeFilteredMcpCatalog = Array.isArray(filteredMcpCatalog) ? filteredMcpCatalog : [];
  const safeMcpExtraHeaders = Array.isArray(mcpExtraHeaders) ? mcpExtraHeaders : [];

  return (
    <>
      <DetailDrawer
        open={githubMcpGuideOpen}
        onClose={() => setGithubMcpGuideOpen(false)}
        title="Official GitHub MCP guide"
      >
        <div className="grid gap-3">
          <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
            Recommended for Bug Monitor: use the official GitHub MCP endpoint instead of a
            third-party wrapper when you want stable issue read/write operations.
          </div>

          <div className="grid gap-2 md:grid-cols-2">
            <div className="tcp-list-item">
              <div className="text-sm font-medium">Transport URL</div>
              <div className="mt-1 break-all text-sm">https://api.githubcopilot.com/mcp/</div>
              <div className="tcp-subtle text-xs">
                Use this as the MCP server transport in Tandem Settings.
              </div>
            </div>
            <div className="tcp-list-item">
              <div className="text-sm font-medium">Auth mode</div>
              <div className="mt-1 text-sm">Authorization Bearer</div>
              <div className="tcp-subtle text-xs">
                Paste a GitHub token in the MCP server dialog and use bearer auth.
              </div>
            </div>
          </div>

          <div className="grid gap-2">
            <div className="text-sm font-medium">Recommended setup</div>
            <div className="tcp-list-item text-sm">
              1. Open `Add MCP server`.
              <br />
              2. Name it `github` or another stable name.
              <br />
              3. Set transport to `https://api.githubcopilot.com/mcp/`.
              <br />
              4. Set auth mode to `Authorization Bearer`.
              <br />
              5. Paste a GitHub Personal Access Token.
              <br />
              6. Save, connect, then select that MCP server in Bug Monitor settings.
            </div>
          </div>

          <div className="grid gap-2">
            <div className="text-sm font-medium">Token guidance</div>
            <div className="tcp-list-item text-sm">
              For failure reporting, the token needs issue read/write access on the target
              repository so the runtime can create issues and add comments.
            </div>
          </div>

          <div className="grid gap-2">
            <div className="text-sm font-medium">Direct links</div>
            <div className="flex flex-wrap gap-2">
              <a
                className="tcp-btn"
                href="https://github.com/github/github-mcp-server?tab=readme-ov-file"
                target="_blank"
                rel="noreferrer"
              >
                <i data-lucide="external-link"></i>
                GitHub MCP README
              </a>
              <a
                className="tcp-btn"
                href="https://docs.github.com/en/copilot/how-tos/provide-context/use-mcp/use-the-github-mcp-server"
                target="_blank"
                rel="noreferrer"
              >
                <i data-lucide="external-link"></i>
                GitHub Docs
              </a>
            </div>
          </div>

          <div className="grid gap-2">
            <div className="text-sm font-medium">Issue tools to expect</div>
            <div className="tcp-list-item text-sm">
              The reporter should be able to resolve issue-list, issue-read, issue-create, and
              issue-comment operations from the selected GitHub MCP server. If readiness still
              fails, compare the discovered MCP tools shown in Settings against those issue
              operations.
            </div>
          </div>
        </div>
      </DetailDrawer>

      <AnimatePresence>
        {bugMonitorWorkspaceBrowserOpen ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <button
              type="button"
              className="tcp-confirm-backdrop"
              aria-label="Close Bug Monitor workspace dialog"
              onClick={() => {
                setBugMonitorWorkspaceBrowserOpen(false);
                setBugMonitorWorkspaceBrowserSearch("");
              }}
            />
            <motion.div
              className="tcp-confirm-dialog max-w-2xl"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
            >
              <h3 className="tcp-confirm-title">Select Bug Monitor Directory</h3>
              <p className="tcp-confirm-message">Current: {bugMonitorCurrentBrowseDir || "n/a"}</p>
              <div className="mb-3 rounded-xl border border-white/10 bg-black/20 p-3 text-xs">
                <div className="font-semibold text-slate-100">Where should I go?</div>
                <div className="tcp-subtle mt-1">
                  Hosted installs share Coder repositories at <code>{HOSTED_CODER_REPO_ROOT}</code>.
                  Choose the repo folder, for example{" "}
                  <code>{bugMonitorSuggestedWorkspaceRoot}</code>. The{" "}
                  <code>{HOSTED_TANDEM_DATA_ROOT}</code> folder is runtime state, not the source
                  checkout.
                </div>
              </div>
              <div className="mb-2 flex flex-wrap gap-2">
                <button
                  className="tcp-btn"
                  onClick={() => {
                    if (!bugMonitorWorkspaceParentDir) return;
                    setBugMonitorWorkspaceBrowserDir(bugMonitorWorkspaceParentDir);
                  }}
                  disabled={!bugMonitorWorkspaceParentDir}
                >
                  <i data-lucide="arrow-up-circle"></i>
                  Up
                </button>
                <button
                  className="tcp-btn"
                  onClick={() => {
                    setBugMonitorWorkspaceBrowserDir(HOSTED_CODER_REPO_ROOT);
                    setBugMonitorWorkspaceBrowserSearch("");
                  }}
                >
                  <i data-lucide="folder-git-2"></i>
                  Synced repos
                </button>
                <button
                  className="tcp-btn-primary"
                  onClick={() => {
                    if (!bugMonitorCurrentBrowseDir) return;
                    setBugMonitorWorkspaceRoot(bugMonitorCurrentBrowseDir);
                    setBugMonitorWorkspaceBrowserOpen(false);
                    setBugMonitorWorkspaceBrowserSearch("");
                    toast("ok", `Bug Monitor directory selected: ${bugMonitorCurrentBrowseDir}`);
                  }}
                >
                  <i data-lucide="badge-check"></i>
                  Select This Folder
                </button>
                <button
                  className="tcp-btn"
                  onClick={() => {
                    setBugMonitorWorkspaceBrowserOpen(false);
                    setBugMonitorWorkspaceBrowserSearch("");
                  }}
                >
                  <i data-lucide="x"></i>
                  Close
                </button>
              </div>
              <div className="mb-2">
                <input
                  className="tcp-input"
                  placeholder="Type to filter folders..."
                  value={bugMonitorWorkspaceBrowserSearch}
                  onInput={(e) =>
                    setBugMonitorWorkspaceBrowserSearch((e.target as HTMLInputElement).value)
                  }
                />
              </div>
              <div className="max-h-[360px] overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-2">
                {safeFilteredBugMonitorWorkspaceDirectories.length ? (
                  safeFilteredBugMonitorWorkspaceDirectories.map((entry: any) => {
                    const entryPath = String(entry?.path || "");
                    const hint = hostedWorkspaceDirectoryHint(entryPath);
                    return (
                      <button
                        key={String(entry?.path || entry?.name)}
                        className="tcp-list-item mb-1 w-full text-left"
                        onClick={() => setBugMonitorWorkspaceBrowserDir(entryPath)}
                      >
                        <i data-lucide="folder-open"></i>
                        <span className="min-w-0">
                          <span className="block truncate">
                            {String(entry?.name || entry?.path || "")}
                          </span>
                          {hint ? <span className="tcp-subtle block text-xs">{hint}</span> : null}
                        </span>
                      </button>
                    );
                  })
                ) : (
                  <EmptyState
                    text={
                      bugMonitorWorkspaceSearchQuery
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

      <DetailDrawer
        open={diagnosticsOpen}
        onClose={() => setDiagnosticsOpen(false)}
        title="Browser diagnostics"
      >
        <div className="grid gap-3">
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
            <button
              className="tcp-btn"
              onClick={() =>
                api("/api/engine/browser/status", { method: "GET" })
                  .then(() => toast("ok", "Browser diagnostics refreshed."))
                  .catch((error) =>
                    toast("err", error instanceof Error ? error.message : String(error))
                  )
              }
            >
              <i data-lucide="activity"></i>
              Re-run diagnostics
            </button>
          </Toolbar>

          {browserStatus.isLoading ? (
            <EmptyState text="Loading browser diagnostics..." />
          ) : browserStatus.data ? (
            <>
              {safeBrowserIssues.length ? (
                <div className="grid gap-2">
                  <div className="text-sm font-medium">Blocking issues</div>
                  {safeBrowserIssues.map((issue, index) => (
                    <div key={`${issue.code || "issue"}-${index}`} className="tcp-list-item">
                      <div className="text-sm font-medium">{issue.code || "browser_issue"}</div>
                      <div className="tcp-subtle text-xs">
                        {issue.message || "Unknown browser issue."}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
                  Browser automation is ready on this machine.
                </div>
              )}

              {browserSmokeResult ? (
                <div className="grid gap-2">
                  <div className="text-sm font-medium">Latest smoke test</div>
                  <div className="tcp-list-item">
                    <div className="text-sm font-medium">
                      {browserSmokeResult.title || "Smoke test"}
                    </div>
                    <div className="tcp-subtle text-xs">
                      {browserSmokeResult.final_url || browserSmokeResult.url || "No URL returned"}
                    </div>
                    <div className="tcp-subtle text-xs">
                      Load state: {browserSmokeResult.load_state || "unknown"} · elements:{" "}
                      {String(browserSmokeResult.element_count ?? 0)} · closed:{" "}
                      {browserSmokeResult.closed ? "yes" : "no"}
                    </div>
                    {browserSmokeResult.excerpt ? (
                      <pre className="tcp-code mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words">
                        {browserSmokeResult.excerpt}
                      </pre>
                    ) : null}
                  </div>
                </div>
              ) : null}

              {safeBrowserRecommendations.length ? (
                <div className="grid gap-2">
                  <div className="text-sm font-medium">Recommendations</div>
                  {safeBrowserRecommendations.map((row, index) => (
                    <div key={`browser-recommendation-${index}`} className="tcp-list-item text-sm">
                      {row}
                    </div>
                  ))}
                </div>
              ) : null}

              {safeBrowserInstallHints.length ? (
                <div className="grid gap-2">
                  <div className="text-sm font-medium">Install hints</div>
                  {safeBrowserInstallHints.map((row, index) => (
                    <div key={`browser-install-hint-${index}`} className="tcp-list-item text-sm">
                      {row}
                    </div>
                  ))}
                </div>
              ) : null}

              {browserStatus.data?.last_error ? (
                <div className="tcp-subtle rounded-lg border border-slate-700/60 bg-slate-900/20 p-3 text-xs">
                  Last error: {browserStatus.data.last_error}
                </div>
              ) : null}
            </>
          ) : (
            <EmptyState text="Browser diagnostics are unavailable." />
          )}
        </div>
      </DetailDrawer>

      <AnimatePresence>
        {mcpModalOpen ? (
          <motion.div
            className="tcp-confirm-overlay"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <button
              type="button"
              className="tcp-confirm-backdrop"
              aria-label="Close MCP server dialog"
              onClick={() => setMcpModalOpen(false)}
            />
            <motion.div
              className="tcp-confirm-dialog tcp-verification-modal"
              initial={{ opacity: 0, y: 8, scale: 0.98 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 6, scale: 0.98 }}
            >
              <div className="mb-3 flex items-start justify-between gap-3">
                <div>
                  <h3 className="tcp-confirm-title">
                    {mcpEditingName ? "Edit MCP Server" : "Add MCP Server"}
                  </h3>
                  <p className="tcp-confirm-message">
                    Configure transport and auth without leaving Settings.
                  </p>
                </div>
                <button
                  type="button"
                  className="tcp-btn h-8 px-2"
                  onClick={() => setMcpModalOpen(false)}
                >
                  <i data-lucide="x"></i>
                </button>
              </div>

              <form
                className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden"
                onSubmit={(event) => {
                  event.preventDefault();
                  mcpSaveMutation.mutate();
                }}
              >
                <div className="tcp-settings-tabs">
                  <button
                    type="button"
                    className={`tcp-settings-tab tcp-settings-tab-underline ${
                      mcpModalTab === "catalog" ? "active" : ""
                    }`}
                    onClick={() => setMcpModalTab("catalog")}
                  >
                    <i data-lucide="blocks"></i>
                    Built-in catalog
                  </button>
                  <button
                    type="button"
                    className={`tcp-settings-tab tcp-settings-tab-underline ${
                      mcpModalTab === "manual" ? "active" : ""
                    }`}
                    onClick={() => setMcpModalTab("manual")}
                  >
                    <i data-lucide="square-pen"></i>
                    Manual
                  </button>
                </div>

                {mcpModalTab === "catalog" ? (
                  <div className="grid min-h-0 flex-1 content-start gap-3 overflow-hidden">
                    <div className="flex items-center justify-between gap-3">
                      <div className="tcp-subtle text-sm">
                        {mcpCatalog.generatedAt
                          ? `Built-in MCP catalog · generated ${mcpCatalog.generatedAt}`
                          : "Built-in MCP catalog"}
                      </div>
                      <button
                        type="button"
                        className="tcp-btn h-8 px-3 text-xs"
                        onClick={() => void mcpCatalogQuery.refetch()}
                      >
                        <i data-lucide="refresh-cw"></i>
                        Refresh
                      </button>
                    </div>
                    <input
                      className="tcp-input"
                      value={mcpCatalogSearch}
                      onInput={(event) =>
                        setMcpCatalogSearch((event.target as HTMLInputElement).value)
                      }
                      placeholder="Search built-in MCP providers"
                    />
                    <div className="grid min-h-0 flex-1 auto-rows-max content-start gap-2 overflow-y-auto pr-1 md:grid-cols-2">
                      {safeFilteredMcpCatalog.length ? (
                        safeFilteredMcpCatalog.map((row) => {
                          const alreadyConfigured = configuredMcpServerNames.has(
                            String(row.serverConfigName || row.slug || "").toLowerCase()
                          );
                          return (
                            <div
                              key={row.slug}
                              className="tcp-list-item grid h-full min-h-[8.5rem] content-start gap-2"
                            >
                              <div className="flex flex-wrap items-start justify-between gap-2">
                                <div>
                                  <div className="font-semibold">{row.name}</div>
                                  <div className="tcp-subtle text-xs">
                                    {row.slug}
                                    {row.requiresSetup ? " · setup required" : ""}
                                  </div>
                                </div>
                                <div className="flex flex-wrap gap-2">
                                  <Badge tone="info">{row.toolCount} tools</Badge>
                                  {row.authKind === "oauth" ? (
                                    <Badge tone="info">OAuth</Badge>
                                  ) : (
                                    <Badge tone={row.requiresAuth ? "warn" : "ok"}>
                                      {row.requiresAuth ? "Auth" : "Authless"}
                                    </Badge>
                                  )}
                                </div>
                              </div>
                              <div className="tcp-subtle line-clamp-2 text-xs">
                                {row.description || row.transportUrl}
                              </div>
                              <div className="tcp-subtle break-all text-xs">{row.transportUrl}</div>
                              {row.authKind === "oauth" ? (
                                <div className="rounded-xl border border-sky-700/40 bg-sky-950/20 px-3 py-2 text-xs text-sky-100">
                                  Save this provider to start browser sign-in. Tandem will keep the
                                  MCP in a pending state until the authorization completes.
                                </div>
                              ) : null}
                              <div className="mt-auto flex flex-wrap gap-2">
                                <button
                                  type="button"
                                  className="tcp-btn h-8 px-3 text-xs"
                                  onClick={() => {
                                    const nextTransport = row.transportUrl;
                                    const nextName = normalizeMcpName(
                                      row.serverConfigName || row.slug || row.name
                                    );
                                    setMcpName(nextName);
                                    setMcpTransport(nextTransport);
                                    setMcpAuthMode(
                                      row.authKind === "oauth"
                                        ? "oauth"
                                        : nextName === "github" ||
                                            isGithubCopilotMcpTransport(nextTransport)
                                          ? "bearer"
                                          : "none"
                                    );
                                    if (row.authKind === "oauth") setMcpToken("");
                                    setMcpGithubToolsets(
                                      nextName === "github" ||
                                        isGithubCopilotMcpTransport(nextTransport)
                                        ? "default"
                                        : ""
                                    );
                                    setMcpExtraHeaders([]);
                                    setMcpModalTab("manual");
                                    toast(
                                      "ok",
                                      row.authKind === "oauth"
                                        ? `Prefilled ${row.name}. Save to start browser sign-in.`
                                        : `Prefilled ${row.name}. Review and save when ready.`
                                    );
                                  }}
                                >
                                  Prefill
                                </button>
                                {row.documentationUrl ? (
                                  <a
                                    className="tcp-btn h-8 px-3 text-xs"
                                    href={row.documentationUrl}
                                    target="_blank"
                                    rel="noreferrer"
                                  >
                                    <i data-lucide="external-link"></i>
                                    Docs
                                  </a>
                                ) : null}
                                {alreadyConfigured ? <Badge tone="ok">added</Badge> : null}
                              </div>
                            </div>
                          );
                        })
                      ) : (
                        <EmptyState text="No built-in MCP providers match this search." />
                      )}
                    </div>
                  </div>
                ) : (
                  <>
                    <div className="grid gap-3 md:grid-cols-2">
                      <div className="grid gap-2">
                        <label className="text-sm font-medium">Name</label>
                        <input
                          className="tcp-input"
                          value={mcpName}
                          onInput={(event) => setMcpName((event.target as HTMLInputElement).value)}
                          placeholder="mcp-server"
                        />
                      </div>
                      <div className="grid gap-2">
                        <label className="text-sm font-medium">Auth mode</label>
                        <select
                          className="tcp-select"
                          value={mcpAuthMode}
                          onChange={(event) => {
                            const nextMode = (event.target as HTMLSelectElement).value;
                            setMcpAuthMode(nextMode);
                            if (nextMode === "oauth") setMcpToken("");
                          }}
                        >
                          <option value="none">No Auth Header</option>
                          <option value="auto">Auto</option>
                          <option value="oauth">OAuth</option>
                          <option value="x-api-key">x-api-key</option>
                          <option value="bearer">Authorization Bearer</option>
                          <option value="custom">Custom Header</option>
                        </select>
                      </div>
                    </div>

                    <div className="grid gap-2">
                      <label className="text-sm font-medium">Transport URL</label>
                      <input
                        className="tcp-input"
                        value={mcpTransport}
                        onInput={(event) => {
                          const value = (event.target as HTMLInputElement).value;
                          setMcpTransport(value);
                          if (
                            isGithubCopilotMcpTransport(value) &&
                            !String(mcpGithubToolsets || "").trim()
                          ) {
                            setMcpGithubToolsets("default");
                          }
                          if (!String(mcpName || "").trim() || mcpName === "mcp-server") {
                            const inferred = inferMcpNameFromTransport(value);
                            if (inferred) setMcpName(inferred);
                          }
                          const inferredAuthKind = inferMcpCatalogAuthKind(
                            mcpCatalog,
                            mcpName,
                            value
                          );
                          if (
                            inferredAuthKind === "oauth" &&
                            (mcpAuthMode === "none" || mcpAuthMode === "auto")
                          ) {
                            setMcpAuthMode("oauth");
                            setMcpToken("");
                          }
                        }}
                        placeholder="https://example.com/mcp"
                      />
                    </div>

                    {mcpAuthMode === "custom" ? (
                      <div className="grid gap-2">
                        <label className="text-sm font-medium">Custom header name</label>
                        <input
                          className="tcp-input"
                          value={mcpCustomHeader}
                          onInput={(event) =>
                            setMcpCustomHeader((event.target as HTMLInputElement).value)
                          }
                          placeholder="X-My-Token"
                        />
                      </div>
                    ) : null}

                    {mcpAuthMode === "oauth" ? (
                      <div className="grid gap-2 rounded-xl border border-sky-700/50 bg-sky-950/20 px-3 py-3 text-xs text-sky-100">
                        <div className="font-medium">OAuth sign-in flow</div>
                        <div>{mcpOauthGuidanceText}</div>
                        <div className="tcp-subtle text-xs text-sky-100/80">
                          {mcpOauthStartsAfterSave
                            ? "Saving this server will immediately start the browser handoff."
                            : "Turn on `Connect after save` to launch the authorization flow as soon as the server is saved."}
                        </div>
                        <div className="tcp-subtle text-xs text-sky-100/80">
                          {mcpAuthPreviewText}
                        </div>
                      </div>
                    ) : (
                      <div className="grid gap-2">
                        <label className="text-sm font-medium">Token</label>
                        <input
                          className="tcp-input"
                          type="password"
                          value={mcpToken}
                          onInput={(event) => setMcpToken((event.target as HTMLInputElement).value)}
                          placeholder="token"
                        />
                        <div className="tcp-subtle text-xs">{mcpAuthPreviewText}</div>
                      </div>
                    )}

                    {mcpIsGithubTransport ? (
                      <div className="grid gap-2">
                        <label className="text-sm font-medium">GitHub toolsets</label>
                        <input
                          className="tcp-input"
                          value={mcpGithubToolsets}
                          onInput={(event) =>
                            setMcpGithubToolsets((event.target as HTMLInputElement).value)
                          }
                          placeholder="default,projects"
                        />
                        <div className="tcp-subtle text-xs">
                          Sent as `X-MCP-Toolsets`. Built-in GitHub starts with `default`; add
                          values like `projects`, `issues`, or `pull_requests`.
                        </div>
                      </div>
                    ) : null}

                    <div className="grid gap-2">
                      <div className="flex items-center justify-between gap-2">
                        <label className="text-sm font-medium">Additional headers</label>
                        <button
                          type="button"
                          className="tcp-btn h-8 px-3 text-xs"
                          onClick={() =>
                            setMcpExtraHeaders((prev) => [
                              ...(Array.isArray(prev) ? prev : []),
                              { key: "", value: "" },
                            ])
                          }
                        >
                          <i data-lucide="plus"></i>
                          Add header
                        </button>
                      </div>
                      {safeMcpExtraHeaders.length ? (
                        <div className="grid gap-2">
                          {safeMcpExtraHeaders.map((row, index) => (
                            <div
                              key={`mcp-header-${index}`}
                              className="grid gap-2 md:grid-cols-[1fr_1fr_auto]"
                            >
                              <input
                                className="tcp-input"
                                value={row.key}
                                onInput={(event) =>
                                  setMcpExtraHeaders((prev) =>
                                    (Array.isArray(prev) ? prev : []).map((entry, entryIndex) =>
                                      entryIndex === index
                                        ? {
                                            ...entry,
                                            key: (event.target as HTMLInputElement).value,
                                          }
                                        : entry
                                    )
                                  )
                                }
                                placeholder="Header name"
                              />
                              <input
                                className="tcp-input"
                                value={row.value}
                                onInput={(event) =>
                                  setMcpExtraHeaders((prev) =>
                                    (Array.isArray(prev) ? prev : []).map((entry, entryIndex) =>
                                      entryIndex === index
                                        ? {
                                            ...entry,
                                            value: (event.target as HTMLInputElement).value,
                                          }
                                        : entry
                                    )
                                  )
                                }
                                placeholder="Header value"
                              />
                              <button
                                type="button"
                                className="tcp-btn"
                                onClick={() =>
                                  setMcpExtraHeaders((prev) =>
                                    (Array.isArray(prev) ? prev : []).filter(
                                      (_, entryIndex) => entryIndex !== index
                                    )
                                  )
                                }
                              >
                                Remove
                              </button>
                            </div>
                          ))}
                        </div>
                      ) : (
                        <div className="tcp-subtle text-xs">
                          Add arbitrary request headers such as `X-MCP-Insiders` or vendor feature
                          flags.
                        </div>
                      )}
                    </div>

                    <label className="inline-flex items-center gap-2 text-sm text-slate-200">
                      <input
                        type="checkbox"
                        className="accent-slate-400"
                        checked={mcpConnectAfterAdd}
                        onChange={(event) =>
                          setMcpConnectAfterAdd((event.target as HTMLInputElement).checked)
                        }
                      />
                      {mcpAuthMode === "oauth" ? "Start sign-in after save" : "Connect after save"}
                    </label>
                  </>
                )}

                <div className="tcp-confirm-actions mt-2">
                  <button type="button" className="tcp-btn" onClick={() => setMcpModalOpen(false)}>
                    Cancel
                  </button>
                  <button
                    type="submit"
                    className="tcp-btn-primary"
                    disabled={mcpSaveMutation.isPending}
                  >
                    <i data-lucide="save"></i>
                    {mcpOauthStartsAfterSave
                      ? "Save MCP server and start sign-in"
                      : "Save MCP server"}
                  </button>
                </div>
              </form>
            </motion.div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </>
  );
}
