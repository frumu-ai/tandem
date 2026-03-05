import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

type BrowserBlockingIssue = {
  code?: string;
  message?: string;
};

type BrowserBinaryStatus = {
  found?: boolean;
  path?: string | null;
  version?: string | null;
  channel?: string | null;
};

type BrowserStatusResponse = {
  enabled?: boolean;
  runnable?: boolean;
  headless_default?: boolean;
  sidecar?: BrowserBinaryStatus;
  browser?: BrowserBinaryStatus;
  blocking_issues?: BrowserBlockingIssue[];
  recommendations?: string[];
  install_hints?: string[];
  last_error?: string | null;
};

export function SettingsPage({
  client,
  api,
  toast,
  identity,
  themes,
  setTheme,
  themeId,
  refreshProviderStatus,
  refreshIdentityStatus,
}: AppPageProps) {
  const queryClient = useQueryClient();
  const [modelSearchByProvider, setModelSearchByProvider] = useState<Record<string, string>>({});
  const [botName, setBotName] = useState(String(identity?.botName || "Tandem"));
  const [botAvatarUrl, setBotAvatarUrl] = useState(String(identity?.botAvatarUrl || ""));
  const [botControlPanelAlias, setBotControlPanelAlias] = useState("Control Center");
  const avatarInputRef = useRef<HTMLInputElement | null>(null);

  const loadIdentityConfig = async () => {
    const identityApi = (client as any)?.identity;
    if (identityApi?.get) return identityApi.get();
    return api("/api/engine/config/identity", { method: "GET" });
  };

  const patchIdentityConfig = async (payload: any) => {
    const identityApi = (client as any)?.identity;
    if (identityApi?.patch) return identityApi.patch(payload);
    return api("/api/engine/config/identity", {
      method: "PATCH",
      body: JSON.stringify(payload),
    });
  };

  const identityConfig = useQuery({
    queryKey: ["settings", "identity", "config"],
    queryFn: () => loadIdentityConfig().catch(() => ({ identity: {} as any })),
  });

  useEffect(() => {
    const bot = (identityConfig.data as any)?.identity?.bot || {};
    const aliases = bot?.aliases || {};
    const canonical = String(
      bot?.canonicalName || bot?.canonical_name || identity?.botName || "Tandem"
    ).trim();
    const avatar = String(bot?.avatarUrl || bot?.avatar_url || identity?.botAvatarUrl || "").trim();
    const controlPanelAlias = String(aliases?.controlPanel || aliases?.control_panel || "").trim();
    setBotName(canonical || "Tandem");
    setBotAvatarUrl(avatar);
    setBotControlPanelAlias(controlPanelAlias || "Control Center");
  }, [identity?.botAvatarUrl, identity?.botName, identityConfig.data]);
  const providersCatalog = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [], connected: [] })),
  });
  const providersConfig = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({ default: "", providers: {} })),
  });
  const browserStatus = useQuery<BrowserStatusResponse | null>({
    queryKey: ["settings", "browser", "status"],
    queryFn: () => api("/api/engine/browser/status", { method: "GET" }).catch(() => null),
    refetchInterval: 30_000,
  });
  const [selectedProvider, setSelectedProvider] = [
    String(providersConfig.data?.default || ""),
    () => undefined,
  ] as any;

  const setDefaultsMutation = useMutation({
    mutationFn: async ({ providerId, modelId }: { providerId: string; modelId: string }) =>
      client.providers.setDefaults(providerId, modelId),
    onSuccess: async () => {
      toast("ok", "Updated provider defaults.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
        refreshProviderStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const setApiKeyMutation = useMutation({
    mutationFn: ({ providerId, apiKey }: { providerId: string; apiKey: string }) =>
      client.providers.setApiKey(providerId, apiKey),
    onSuccess: async () => {
      toast("ok", "API key updated.");
      await refreshProviderStatus();
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const saveIdentityMutation = useMutation({
    mutationFn: async () => {
      const currentBot = (identityConfig.data as any)?.identity?.bot || {};
      const currentAliases = currentBot?.aliases || {};
      const canonical = String(botName || "").trim();
      if (!canonical) throw new Error("Bot name is required.");
      const avatar = String(botAvatarUrl || "").trim();
      const controlPanelAlias = String(botControlPanelAlias || "").trim();
      return patchIdentityConfig({
        identity: {
          bot: {
            canonical_name: canonical,
            avatar_url: avatar || null,
            aliases: {
              ...currentAliases,
              control_panel: controlPanelAlias || undefined,
            },
          },
        },
      } as any);
    },
    onSuccess: async () => {
      toast("ok", "Identity updated.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "identity"] }),
        refreshIdentityStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const handleAvatarUpload = (file: File | null) => {
    if (!file) return;
    if (file.size > 10 * 1024 * 1024) {
      toast("err", "Avatar image is too large (max 10 MB).");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const value = typeof reader.result === "string" ? reader.result : "";
      if (!value) {
        toast("err", "Failed to read avatar image.");
        return;
      }
      setBotAvatarUrl(value);
    };
    reader.onerror = () => toast("err", "Failed to read avatar image.");
    reader.readAsDataURL(file);
  };

  const providers = Array.isArray(providersCatalog.data?.all) ? providersCatalog.data.all : [];
  const browserIssues = Array.isArray(browserStatus.data?.blocking_issues)
    ? browserStatus.data?.blocking_issues || []
    : [];
  const browserRecommendations = Array.isArray(browserStatus.data?.recommendations)
    ? browserStatus.data?.recommendations || []
    : [];
  const browserInstallHints = Array.isArray(browserStatus.data?.install_hints)
    ? browserStatus.data?.install_hints || []
    : [];

  const applyDefaultModel = (providerId: string, modelId: string) => {
    const next = String(modelId || "").trim();
    if (!next) return;
    setDefaultsMutation.mutate({ providerId, modelId: next });
  };

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <PageCard title="Provider Setup" subtitle="Default provider/model and API keys">
        <div className="grid gap-2">
          {providers.length ? (
            providers.map((provider: any) => {
              const providerId = String(provider?.id || "");
              const models = Object.keys(provider?.models || {});
              const defaultModel = String(
                providersConfig.data?.providers?.[providerId]?.default_model || models[0] || ""
              );
              const typedModel = String(modelSearchByProvider[providerId] ?? defaultModel).trim();
              const normalizedTyped = typedModel.toLowerCase();
              const filteredModels = models
                .filter((modelId) =>
                  normalizedTyped ? modelId.toLowerCase().includes(normalizedTyped) : true
                )
                .slice(0, 80);
              return (
                <details key={providerId} className="tcp-list-item">
                  <summary className="cursor-pointer font-medium">{providerId}</summary>
                  <div className="mt-2 grid gap-2">
                    <form
                      className="grid gap-2"
                      onSubmit={(e) => {
                        e.preventDefault();
                        applyDefaultModel(providerId, typedModel);
                      }}
                    >
                      <div className="flex gap-2">
                        <input
                          className="tcp-input"
                          value={typedModel}
                          placeholder={`Type model id for ${providerId}`}
                          onInput={(e) =>
                            setModelSearchByProvider((prev) => ({
                              ...prev,
                              [providerId]: (e.target as HTMLInputElement).value,
                            }))
                          }
                        />
                        <button className="tcp-btn" type="submit">
                          <i data-lucide="badge-check"></i>
                          Apply
                        </button>
                      </div>
                      <div className="max-h-48 overflow-auto rounded-lg border border-slate-700/60 bg-slate-900/20 p-1">
                        {filteredModels.length ? (
                          filteredModels.map((modelId) => (
                            <button
                              key={modelId}
                              type="button"
                              className={`block w-full rounded px-2 py-1 text-left text-sm hover:bg-slate-700/30 ${
                                modelId === defaultModel ? "bg-slate-700/40" : ""
                              }`}
                              onClick={() => {
                                setModelSearchByProvider((prev) => ({
                                  ...prev,
                                  [providerId]: modelId,
                                }));
                                applyDefaultModel(providerId, modelId);
                              }}
                            >
                              {modelId}
                            </button>
                          ))
                        ) : (
                          <div className="tcp-subtle px-2 py-1 text-xs">No matching models.</div>
                        )}
                      </div>
                    </form>
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        const input = e.currentTarget.elements.namedItem(
                          "apiKey"
                        ) as HTMLInputElement;
                        const value = String(input?.value || "").trim();
                        if (!value) return;
                        setApiKeyMutation.mutate({ providerId, apiKey: value });
                        input.value = "";
                      }}
                      className="flex gap-2"
                    >
                      <input
                        name="apiKey"
                        className="tcp-input"
                        placeholder={`Set ${providerId} API key`}
                      />
                      <button className="tcp-btn" type="submit">
                        <i data-lucide="save"></i>
                        Save
                      </button>
                    </form>
                  </div>
                </details>
              );
            })
          ) : (
            <EmptyState text="No providers detected." />
          )}
        </div>
      </PageCard>

      <PageCard title="Theme + Identity" subtitle="Control panel look and bot identity">
        <div className="grid gap-3">
          <label className="text-sm tcp-subtle">Theme</label>
          <select
            className="tcp-select"
            value={themeId}
            onChange={(e) => setTheme((e.target as HTMLSelectElement).value)}
          >
            {themes.map((theme: any) => (
              <option key={theme.id} value={theme.id}>
                {theme.name}
              </option>
            ))}
          </select>
          <div className="rounded-lg border border-slate-700/60 bg-slate-900/20 p-3">
            <div className="mb-2 text-sm font-medium">Bot Identity</div>
            <div className="grid gap-2">
              <div className="mb-1 flex items-center justify-between gap-2 rounded-lg border border-slate-700/60 bg-slate-950/30 p-2">
                <div className="inline-flex items-center gap-2">
                  <span className="tcp-brand-avatar inline-grid h-9 w-9 place-items-center overflow-hidden rounded-lg">
                    {botAvatarUrl ? (
                      <img
                        src={botAvatarUrl}
                        alt={botName || "Bot"}
                        className="block h-full w-full object-cover"
                      />
                    ) : (
                      <i data-lucide="cpu"></i>
                    )}
                  </span>
                  <span className="tcp-subtle text-xs">{botName || "Tandem"}</span>
                </div>
                <div className="inline-flex items-center gap-1">
                  <button
                    className="chat-icon-btn h-8 w-8"
                    title="Upload avatar"
                    aria-label="Upload avatar"
                    onClick={() => avatarInputRef.current?.click()}
                  >
                    <i data-lucide="pencil"></i>
                  </button>
                  <button
                    className="chat-icon-btn h-8 w-8"
                    title="Save identity"
                    aria-label="Save identity"
                    onClick={() => saveIdentityMutation.mutate()}
                  >
                    <i data-lucide="save"></i>
                  </button>
                  <button
                    className="chat-icon-btn h-8 w-8"
                    title="Clear avatar"
                    aria-label="Clear avatar"
                    onClick={() => setBotAvatarUrl("")}
                  >
                    <i data-lucide="trash-2"></i>
                  </button>
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
          </div>
          <button
            className="tcp-btn"
            onClick={() => refreshIdentityStatus().then(() => toast("ok", "Identity refreshed."))}
          >
            <i data-lucide="refresh-cw"></i>
            Refresh Identity
          </button>
          <button
            className="tcp-btn"
            onClick={() =>
              refreshProviderStatus().then(() => toast("ok", "Provider status refreshed."))
            }
          >
            <i data-lucide="refresh-cw"></i>
            Refresh Provider Status
          </button>
          <div className="tcp-list-item">
            <div className="text-sm">
              Connected providers: {String(providersCatalog.data?.connected?.length || 0)}
            </div>
            <div className="tcp-subtle text-xs">
              Default: {String(providersConfig.data?.default || "none")}
            </div>
          </div>
        </div>
      </PageCard>

      <PageCard
        title="Browser Diagnostics"
        subtitle="Headless Chromium readiness, detected binaries, and install guidance"
        className="xl:col-span-2"
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

          <div className="flex flex-wrap gap-2">
            <button className="tcp-btn" onClick={() => void browserStatus.refetch()}>
              <i data-lucide="refresh-cw"></i>
              Refresh Browser Status
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
              <i data-lucide="stethoscope"></i>
              Re-run Diagnostics
            </button>
          </div>

          {browserStatus.isLoading ? (
            <EmptyState text="Loading browser diagnostics..." />
          ) : browserStatus.data ? (
            <>
              {browserIssues.length ? (
                <div className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3">
                  <div className="mb-2 text-sm font-medium">Blocking issues</div>
                  <div className="grid gap-2">
                    {browserIssues.map((issue, index) => (
                      <div key={`${issue.code || "issue"}-${index}`} className="tcp-list-item">
                        <div className="text-sm font-medium">{issue.code || "browser_issue"}</div>
                        <div className="tcp-subtle text-xs">
                          {issue.message || "Unknown browser issue."}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ) : (
                <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm">
                  Browser automation is ready on this machine.
                </div>
              )}

              {browserRecommendations.length ? (
                <div className="grid gap-2">
                  <div className="text-sm font-medium">Recommendations</div>
                  {browserRecommendations.map((row, index) => (
                    <div key={`browser-recommendation-${index}`} className="tcp-list-item text-sm">
                      {row}
                    </div>
                  ))}
                </div>
              ) : null}

              {browserInstallHints.length ? (
                <div className="grid gap-2">
                  <div className="text-sm font-medium">Install hints</div>
                  {browserInstallHints.map((row, index) => (
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
      </PageCard>
    </div>
  );
}
