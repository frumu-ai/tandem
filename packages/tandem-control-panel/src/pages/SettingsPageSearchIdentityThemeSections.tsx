import { renderMarkdownSafe } from "../lib/markdown";
import { ThemePicker } from "../ui/ThemePicker.tsx";
import { Badge, PanelCard, Toolbar } from "../ui/index.tsx";
import { EmptyState } from "./ui";
import { useSettingsPageController } from "./SettingsPageController";
import { Icon } from "../ui/Icon";

type SettingsPageControllerState = ReturnType<typeof useSettingsPageController>;

type SettingsPageSearchIdentityThemeSectionsProps = {
  controller: SettingsPageControllerState;
};

export function SettingsPageSearchIdentityThemeSections({
  controller,
}: SettingsPageSearchIdentityThemeSectionsProps) {
  const {
    activeSection,
    avatarInputRef,
    botAvatarUrl,
    botControlPanelAlias,
    botName,
    handleAvatarUpload,
    identity,
    refreshIdentityStatus,
    saveIdentityMutation,
    saveSchedulerSettingsMutation,
    saveSearchSettingsMutation,
    schedulerMaxConcurrent,
    schedulerMode,
    schedulerSettingsQuery,
    searchBackend,
    searchBraveKey,
    searchExaKey,
    searchSearxngUrl,
    searchSettingsQuery,
    searchTandemUrl,
    searchTestQuery,
    searchTestResult,
    searchTimeoutMs,
    setBotAvatarUrl,
    setBotControlPanelAlias,
    setBotName,
    setSchedulerMaxConcurrent,
    setSchedulerMode,
    setSearchBackend,
    setSearchBraveKey,
    setSearchExaKey,
    setSearchSearxngUrl,
    setSearchTandemUrl,
    setSearchTestQuery,
    setSearchTimeoutMs,
    setTheme,
    testSearchMutation,
    themeId,
    themes,
    toast,
  } = controller;

  return (
    <>
      {activeSection === "search" ? (
        <PanelCard
          title="Web Search"
          subtitle="Configure the engine's `websearch` backend and provider keys."
          actions={
            <Toolbar>
              <Badge tone={searchSettingsQuery.data?.settings?.has_brave_key ? "ok" : "warn"}>
                Brave {searchSettingsQuery.data?.settings?.has_brave_key ? "configured" : "missing"}
              </Badge>
              <Badge tone={searchSettingsQuery.data?.settings?.has_exa_key ? "ok" : "warn"}>
                Exa {searchSettingsQuery.data?.settings?.has_exa_key ? "configured" : "missing"}
              </Badge>
              <button
                className="tcp-btn"
                onClick={() =>
                  testSearchMutation.mutate({
                    query: searchTestQuery.trim(),
                  })
                }
                disabled={
                  !searchSettingsQuery.data?.available ||
                  !searchTestQuery.trim() ||
                  testSearchMutation.isPending
                }
              >
                <Icon name={testSearchMutation.isPending ? "loader-circle" : "search"} />
                {testSearchMutation.isPending ? "Testing..." : "Test search"}
              </button>
              <button
                className="tcp-btn-primary"
                onClick={() =>
                  saveSearchSettingsMutation.mutate({
                    backend: searchBackend,
                    tandem_url: searchTandemUrl,
                    searxng_url: searchSearxngUrl,
                    timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                    brave_api_key: searchBraveKey.trim() || undefined,
                    exa_api_key: searchExaKey.trim() || undefined,
                  })
                }
                disabled={
                  !searchSettingsQuery.data?.available || saveSearchSettingsMutation.isPending
                }
              >
                <Icon name="save" />
                Save
              </button>
            </Toolbar>
          }
        >
          {!searchSettingsQuery.data?.available ? (
            <EmptyState
              text={
                searchSettingsQuery.data?.reason ||
                "Search settings are only editable here when the panel points at a local engine host or a Tandem-hosted managed server."
              }
            />
          ) : (
            <div className="grid gap-4">
              <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4 text-sm">
                <div className="font-medium">Engine env file</div>
                <div className="tcp-subtle mt-1 break-all">
                  {searchSettingsQuery.data?.managed_env_path || "/etc/tandem/engine.env"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  {searchSettingsQuery.data?.restart_hint || "Changes apply immediately."}
                </div>
              </div>

              <div className="grid gap-3 md:grid-cols-2">
                <label className="grid gap-1 text-sm">
                  <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Backend</span>
                  <select
                    className="tcp-select"
                    value={searchBackend}
                    onChange={(e) => setSearchBackend((e.target as HTMLSelectElement).value)}
                  >
                    <option value="auto">Auto failover</option>
                    <option value="brave">Brave Search</option>
                    <option value="exa">Exa</option>
                    <option value="searxng">SearxNG</option>
                    <option value="tandem">Tandem hosted search</option>
                    <option value="none">Disable websearch</option>
                  </select>
                </label>
                <label className="grid gap-1 text-sm">
                  <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                    Timeout (ms)
                  </span>
                  <input
                    className="tcp-input"
                    type="number"
                    min={1000}
                    max={120000}
                    value={searchTimeoutMs}
                    onInput={(e) => setSearchTimeoutMs((e.target as HTMLInputElement).value)}
                  />
                </label>
              </div>

              <div className="grid gap-3 md:grid-cols-2">
                <label className="grid gap-1 text-sm">
                  <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                    Tandem search URL
                  </span>
                  <input
                    className="tcp-input"
                    placeholder="https://search.tandem.ac"
                    value={searchTandemUrl}
                    onInput={(e) => setSearchTandemUrl((e.target as HTMLInputElement).value)}
                  />
                  <span className="tcp-subtle text-xs">
                    Only used when backend is set to `tandem` or `auto`. This is the hosted Tandem
                    search router, not the SearXNG endpoint.
                  </span>
                </label>
                <label className="grid gap-1 text-sm">
                  <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                    SearxNG URL
                  </span>
                  <input
                    className="tcp-input"
                    placeholder="http://127.0.0.1:8080"
                    value={searchSearxngUrl}
                    onInput={(e) => setSearchSearxngUrl((e.target as HTMLInputElement).value)}
                  />
                  <span className="tcp-subtle text-xs">
                    Only used when backend is `searxng` or `auto`.
                  </span>
                </label>
              </div>

              <div className="grid gap-3 rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <div className="font-medium">Search test</div>
                    <div className="tcp-subtle mt-1 text-xs">
                      Runs `websearch` against the currently running engine config and renders the
                      result as markdown below.
                    </div>
                  </div>
                  <Badge tone="warn">Tests live engine config</Badge>
                </div>
                <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
                  <input
                    className="tcp-input"
                    placeholder="Try a test query like autonomous AI agentic workflows"
                    value={searchTestQuery}
                    onInput={(e) => setSearchTestQuery((e.target as HTMLInputElement).value)}
                  />
                  <button
                    className="tcp-btn"
                    onClick={() =>
                      testSearchMutation.mutate({
                        query: searchTestQuery.trim(),
                      })
                    }
                    disabled={
                      !searchSettingsQuery.data?.available ||
                      !searchTestQuery.trim() ||
                      testSearchMutation.isPending
                    }
                  >
                    <Icon name={testSearchMutation.isPending ? "loader-circle" : "play"} />
                    {testSearchMutation.isPending ? "Running..." : "Run test"}
                  </button>
                </div>
                {searchTestResult?.markdown ? (
                  <div className="grid gap-2">
                    <div className="flex flex-wrap items-center gap-2 text-xs">
                      <Badge tone="ok">
                        Backend{" "}
                        {String(
                          searchTestResult.parsed_output?.backend ||
                            searchTestResult.metadata?.backend ||
                            "unknown"
                        )}
                      </Badge>
                      {searchTestResult.parsed_output?.configured_backend ? (
                        <Badge tone="info">
                          Configured {String(searchTestResult.parsed_output.configured_backend)}
                        </Badge>
                      ) : null}
                      {searchTestResult.metadata?.error ? (
                        <Badge tone="warn">
                          {String(searchTestResult.metadata.error).replaceAll("_", " ")}
                        </Badge>
                      ) : null}
                    </div>
                    <div
                      className="tcp-markdown tcp-markdown-ai max-h-[320px] overflow-auto rounded-xl border border-slate-700/60 bg-black/20 p-3 text-sm"
                      dangerouslySetInnerHTML={{
                        __html: renderMarkdownSafe(searchTestResult.markdown || ""),
                      }}
                    />
                  </div>
                ) : null}
              </div>

              <div className="grid gap-3 md:grid-cols-2">
                <div className="grid gap-2 rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                  <div className="flex items-center justify-between gap-2">
                    <div className="font-medium">Brave Search key</div>
                    <Badge tone={searchSettingsQuery.data?.settings?.has_brave_key ? "ok" : "warn"}>
                      {searchSettingsQuery.data?.settings?.has_brave_key ? "Saved" : "Missing"}
                    </Badge>
                  </div>
                  <input
                    className="tcp-input"
                    type="password"
                    placeholder="Paste Brave Search key"
                    value={searchBraveKey}
                    onInput={(e) => setSearchBraveKey((e.target as HTMLInputElement).value)}
                  />
                  <div className="flex flex-wrap gap-2">
                    <button
                      className="tcp-btn"
                      onClick={() =>
                        saveSearchSettingsMutation.mutate({
                          backend: searchBackend,
                          tandem_url: searchTandemUrl,
                          searxng_url: searchSearxngUrl,
                          timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                          brave_api_key: searchBraveKey.trim() || undefined,
                        })
                      }
                      disabled={!searchBraveKey.trim() || saveSearchSettingsMutation.isPending}
                    >
                      Save Brave Key
                    </button>
                    {searchSettingsQuery.data?.settings?.has_brave_key ? (
                      <button
                        className="tcp-btn"
                        onClick={() =>
                          saveSearchSettingsMutation.mutate({
                            backend: searchBackend,
                            tandem_url: searchTandemUrl,
                            searxng_url: searchSearxngUrl,
                            timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                            clear_brave_key: true,
                          })
                        }
                        disabled={saveSearchSettingsMutation.isPending}
                      >
                        Remove
                      </button>
                    ) : null}
                  </div>
                </div>

                <div className="grid gap-2 rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                  <div className="flex items-center justify-between gap-2">
                    <div className="font-medium">Exa key</div>
                    <Badge tone={searchSettingsQuery.data?.settings?.has_exa_key ? "ok" : "warn"}>
                      {searchSettingsQuery.data?.settings?.has_exa_key ? "Saved" : "Missing"}
                    </Badge>
                  </div>
                  <input
                    className="tcp-input"
                    type="password"
                    placeholder="Paste Exa API key"
                    value={searchExaKey}
                    onInput={(e) => setSearchExaKey((e.target as HTMLInputElement).value)}
                  />
                  <div className="flex flex-wrap gap-2">
                    <button
                      className="tcp-btn"
                      onClick={() =>
                        saveSearchSettingsMutation.mutate({
                          backend: searchBackend,
                          tandem_url: searchTandemUrl,
                          searxng_url: searchSearxngUrl,
                          timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                          exa_api_key: searchExaKey.trim() || undefined,
                        })
                      }
                      disabled={!searchExaKey.trim() || saveSearchSettingsMutation.isPending}
                    >
                      Save Exa Key
                    </button>
                    {searchSettingsQuery.data?.settings?.has_exa_key ? (
                      <button
                        className="tcp-btn"
                        onClick={() =>
                          saveSearchSettingsMutation.mutate({
                            backend: searchBackend,
                            tandem_url: searchTandemUrl,
                            searxng_url: searchSearxngUrl,
                            timeout_ms: Number.parseInt(searchTimeoutMs || "10000", 10),
                            clear_exa_key: true,
                          })
                        }
                        disabled={saveSearchSettingsMutation.isPending}
                      >
                        Remove
                      </button>
                    ) : null}
                  </div>
                </div>
              </div>

              <div className="tcp-subtle text-xs">
                `auto` tries the configured backends with failover. If Brave is rate-limited and Exa
                is configured, the engine can continue with Exa instead of returning a generic
                unavailable message.
              </div>
            </div>
          )}
        </PanelCard>
      ) : null}

      {activeSection === "scheduler" ? (
        <PanelCard
          title="Automation Scheduler"
          subtitle="Controls parallel execution of automation runs. Restart tandem-engine after changing."
          actions={
            <Toolbar>
              <button
                className="tcp-btn-primary"
                onClick={() =>
                  saveSchedulerSettingsMutation.mutate({
                    mode: schedulerMode,
                    max_concurrent_runs: schedulerMaxConcurrent
                      ? Number.parseInt(schedulerMaxConcurrent, 10)
                      : null,
                  })
                }
                disabled={
                  !schedulerSettingsQuery.data?.available || saveSchedulerSettingsMutation.isPending
                }
              >
                <Icon name="save" />
                Save
              </button>
            </Toolbar>
          }
        >
          {!schedulerSettingsQuery.data?.available ? (
            <EmptyState
              text={
                schedulerSettingsQuery.data?.reason ||
                "Scheduler settings are only editable here when the panel points at a local engine host or a Tandem-hosted managed server."
              }
            />
          ) : (
            <div className="grid gap-4">
              <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4 text-sm">
                <div className="font-medium">Engine env file</div>
                <div className="tcp-subtle mt-1 break-all">
                  {schedulerSettingsQuery.data?.managed_env_path || "/etc/tandem/engine.env"}
                </div>
                <div className="tcp-subtle mt-2 text-xs">
                  {schedulerSettingsQuery.data?.restart_hint ||
                    "Restart tandem-engine after changing scheduler mode."}
                </div>
              </div>

              <div className="grid gap-3 md:grid-cols-2">
                <label className="grid gap-1 text-sm">
                  <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">Mode</span>
                  <select
                    className="tcp-select"
                    value={schedulerMode}
                    onChange={(e) => setSchedulerMode((e.target as HTMLSelectElement).value)}
                  >
                    <option value="multi">Multi — parallel runs (recommended)</option>
                    <option value="single">Single — one run at a time</option>
                  </select>
                </label>
                <label className="grid gap-1 text-sm">
                  <span className="tcp-subtle text-xs uppercase tracking-[0.18em]">
                    Max concurrent runs
                  </span>
                  <input
                    className="tcp-input"
                    type="number"
                    min={1}
                    max={32}
                    placeholder="8 (default)"
                    value={schedulerMaxConcurrent}
                    onInput={(e) => setSchedulerMaxConcurrent((e.target as HTMLInputElement).value)}
                  />
                </label>
              </div>

              <div className="tcp-subtle text-xs">
                Multi mode allows several automation runs to execute concurrently. Max concurrent
                runs caps parallelism. Changes require a tandem-engine restart to take effect.
              </div>
            </div>
          )}
        </PanelCard>
      ) : null}

      {activeSection === "identity" ? (
        <PanelCard
          title="Identity preview"
          subtitle="Live preview of how the assistant appears across the panel."
          actions={
            <Toolbar>
              <button
                className="tcp-btn"
                onClick={() =>
                  refreshIdentityStatus().then(() => toast("ok", "Identity refreshed."))
                }
              >
                <Icon name="refresh-cw" />
                Refresh identity
              </button>
              <button
                className="tcp-btn-primary"
                onClick={() => saveIdentityMutation.mutate()}
                disabled={saveIdentityMutation.isPending}
              >
                <Icon name="save" />
                Save
              </button>
            </Toolbar>
          }
        >
          <div className="grid gap-3">
            <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
              <div className="flex items-center justify-between gap-3">
                <div className="inline-flex items-center gap-3">
                  <span className="tcp-brand-avatar inline-grid h-12 w-12 rounded-xl">
                    <img
                      src={botAvatarUrl || "/icon.png"}
                      alt={botName || "Tandem"}
                      className="block h-full w-full object-contain p-1"
                    />
                  </span>
                  <div>
                    <div className="font-semibold">{botName || "Tandem"}</div>
                    <div className="tcp-subtle text-xs">
                      {botControlPanelAlias || "Control Center"}
                    </div>
                  </div>
                </div>
                <Toolbar>
                  <button
                    className="tcp-icon-btn"
                    title="Upload avatar"
                    aria-label="Upload avatar"
                    onClick={() => avatarInputRef.current?.click()}
                  >
                    <Icon name="pencil" />
                  </button>
                  <button
                    className="tcp-icon-btn"
                    title="Clear avatar"
                    aria-label="Clear avatar"
                    onClick={() => setBotAvatarUrl("")}
                  >
                    <Icon name="trash-2" />
                  </button>
                </Toolbar>
              </div>
            </div>

            <input
              className="tcp-input"
              value={botName}
              onInput={(e) => setBotName((e.target as HTMLInputElement).value)}
              placeholder="Bot name"
            />
            <input
              className="tcp-input"
              value={botControlPanelAlias}
              onInput={(e) => setBotControlPanelAlias((e.target as HTMLInputElement).value)}
              placeholder="Control panel alias"
            />
            <input
              className="tcp-input"
              value={botAvatarUrl}
              onInput={(e) => setBotAvatarUrl((e.target as HTMLInputElement).value)}
              placeholder="Avatar URL or data URL"
            />
            <input
              ref={avatarInputRef}
              type="file"
              accept="image/*"
              className="hidden"
              onChange={(e) =>
                handleAvatarUpload((e.target as HTMLInputElement).files?.[0] || null)
              }
            />
          </div>
        </PanelCard>
      ) : null}

      {activeSection === "theme" ? (
        <PanelCard
          title="Theme studio"
          subtitle="Preview tiles with richer feedback and immediate switching."
        >
          <ThemePicker themes={themes} themeId={themeId} onChange={setTheme} />
        </PanelCard>
      ) : null}
    </>
  );
}
