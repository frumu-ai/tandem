import { AnimatePresence, motion } from "motion/react";
import type { RouteId } from "../app/routes";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import { EmptyState } from "./ui";
import { Badge, PanelCard, Toolbar } from "../ui/index.tsx";
import { providerHints } from "../app/store.js";
import {
  OPENAI_CODEX_PROVIDER_ID,
  providerCatalogBadge,
  providerCatalogSubtitle,
  useSettingsPageController,
} from "./SettingsPageController";

type SettingsPageControllerState = ReturnType<typeof useSettingsPageController>;

type SettingsPageNavigationProvidersSectionsProps = {
  controller: SettingsPageControllerState;
};

export function SettingsPageNavigationProvidersSections({
  controller,
}: SettingsPageNavigationProvidersSectionsProps) {
  const {
    activeSection,
    advancedNavigationRows,
    api,
    applyDefaultModel,
    authorizeProviderOAuthMutation,
    codexAuthFileName,
    codexAuthInputRef,
    codexAuthJsonText,
    connectedProviderCount,
    customConfiguredProviders,
    customProviderApiKey,
    customProviderFormOpen,
    customProviderId,
    customProviderMakeDefault,
    customProviderModel,
    customProviderUrl,
    defaultNavigationRows,
    disconnectProviderOAuthMutation,
    hiddenAdvancedNavigationCount,
    hostedManaged,
    importCodexAuthFile,
    importCodexAuthJsonMutation,
    installConfigError,
    installConfigQuery,
    installConfigText,
    installProfileQuery,
    localEngine,
    modelSearchByProvider,
    navigation,
    navigationRows,
    oauthSessionByProvider,
    providerAuthById,
    providerDefaultsOpen,
    providers,
    providersCatalog,
    providersConfig,
    refreshProviderStatus,
    saveCustomProviderMutation,
    saveInstallConfigMutation,
    setActiveSection,
    setApiKeyMutation,
    setCodexAuthJsonText,
    setCustomProviderApiKey,
    setCustomProviderFormOpen,
    setCustomProviderId,
    setCustomProviderMakeDefault,
    setCustomProviderModel,
    setCustomProviderUrl,
    setDefaultsMutation,
    setInstallConfigText,
    setModelSearchByProvider,
    setNavigationRouteVisibility,
    setOauthSessionByProvider,
    setProviderDefaultsOpen,
    resetNavigationSections,
    toast,
    useLocalCodexSessionMutation,
    visibleNavigationCount,
    showAllNavigationSections,
  } = controller;
  const safeNavigationRows = Array.isArray(navigationRows) ? navigationRows : [];
  const safeDefaultNavigationRows = Array.isArray(defaultNavigationRows)
    ? defaultNavigationRows
    : [];
  const safeAdvancedNavigationRows = Array.isArray(advancedNavigationRows)
    ? advancedNavigationRows
    : [];
  const safeCustomConfiguredProviders = Array.isArray(customConfiguredProviders)
    ? customConfiguredProviders
    : [];
  const safeProviders = Array.isArray(providers) ? providers : [];

  return (
    <>
      {activeSection === "navigation" ? (
        <PanelCard
          title="Sidebar navigation"
          subtitle={
            navigation?.acaMode
              ? "ACA mode keeps Dashboard, Chat, Coder, and Settings visible by default."
              : "Choose which sections appear in the sidebar. Advanced and experimental surfaces stay hidden until you turn them on."
          }
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone={navigation?.acaMode ? "ok" : "info"}>
                {navigation?.acaMode ? "ACA compact default" : "Core-first default"}
              </Badge>
              <Badge tone="ghost">
                {visibleNavigationCount}/{safeNavigationRows.length} visible
              </Badge>
              <button className="tcp-btn" type="button" onClick={() => showAllNavigationSections()}>
                Show all sections
              </button>
              <button
                className="tcp-btn-primary"
                type="button"
                onClick={() => resetNavigationSections()}
              >
                Reset {navigation?.acaMode ? "ACA compact" : "default"}
              </button>
            </div>
          }
        >
          <div className="grid gap-4">
            <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="font-medium">Default sections</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    These sections are part of the standard control panel and start on by default.
                  </div>
                </div>
                <Badge tone="ok">
                  {safeDefaultNavigationRows.filter((row) => row.enabled).length}/
                  {safeDefaultNavigationRows.length} shown
                </Badge>
              </div>
              <div className="mt-3 grid gap-2">
                {safeDefaultNavigationRows.map((row) => (
                  <button
                    key={row.routeId}
                    type="button"
                    aria-pressed={row.enabled}
                    title={`${row.enabled ? "Hide" : "Show"} ${row.label} in the sidebar`}
                    className={`flex items-center justify-between rounded-xl border px-3 py-3 text-left transition ${
                      row.enabled
                        ? "border-lime-500/40 bg-lime-500/10 hover:border-lime-400/70"
                        : "border-slate-700/60 bg-slate-900/20 hover:border-slate-500/70"
                    }`}
                    onClick={() =>
                      setNavigationRouteVisibility(row.routeId as RouteId, !row.enabled)
                    }
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <span
                        className={`flex h-9 w-9 items-center justify-center rounded-lg border ${
                          row.enabled
                            ? "border-lime-500/30 bg-lime-500/10 text-lime-200"
                            : "border-slate-700/70 bg-slate-950/30 text-slate-300"
                        }`}
                      >
                        <i data-lucide={row.icon}></i>
                      </span>
                      <div className="min-w-0">
                        <div className="font-medium">{row.label}</div>
                        <div className="tcp-subtle truncate text-xs">{row.description}</div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      {row.defaultVisible ? <Badge tone="info">Default</Badge> : null}
                      <Badge tone={row.enabled ? "warn" : "ok"}>
                        {row.enabled ? "Hide" : "Show"}
                      </Badge>
                    </div>
                  </button>
                ))}
              </div>
            </div>

            <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="font-medium">Advanced / experimental sections</div>
                  <div className="tcp-subtle mt-1 text-xs">
                    Only routes that ship hidden by default live here.
                  </div>
                </div>
                <Badge tone={hiddenAdvancedNavigationCount > 0 ? "warn" : "ok"}>
                  {hiddenAdvancedNavigationCount} hidden
                </Badge>
              </div>
              <div className="mt-3 grid gap-2">
                {safeAdvancedNavigationRows.map((row) => (
                  <button
                    key={row.routeId}
                    type="button"
                    aria-pressed={row.enabled}
                    title={`${row.enabled ? "Hide" : "Show"} ${row.label} in the sidebar`}
                    className={`flex items-center justify-between rounded-xl border px-3 py-3 text-left transition ${
                      row.enabled
                        ? "border-lime-500/40 bg-lime-500/10 hover:border-lime-400/70"
                        : "border-slate-700/60 bg-slate-900/20 hover:border-slate-500/70"
                    }`}
                    onClick={() =>
                      setNavigationRouteVisibility(row.routeId as RouteId, !row.enabled)
                    }
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <span
                        className={`flex h-9 w-9 items-center justify-center rounded-lg border ${
                          row.enabled
                            ? "border-lime-500/30 bg-lime-500/10 text-lime-200"
                            : "border-slate-700/70 bg-slate-950/30 text-slate-300"
                        }`}
                      >
                        <i data-lucide={row.icon}></i>
                      </span>
                      <div className="min-w-0">
                        <div className="font-medium">{row.label}</div>
                        <div className="tcp-subtle truncate text-xs">{row.description}</div>
                      </div>
                    </div>
                    <Badge tone={row.enabled ? "warn" : "ok"}>
                      {row.enabled ? "Hide" : "Show"}
                    </Badge>
                  </button>
                ))}
              </div>
            </div>

            <div className="tcp-subtle text-xs">
              These preferences are stored in this browser only.
            </div>
          </div>
        </PanelCard>
      ) : null}

      {activeSection === "install" ? (
        <PanelCard
          title="Install config"
          subtitle="Durable non-secret install preferences stored in tandem-data for Tandem startup and navigation defaults."
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge
                tone={
                  String(installProfileQuery.data?.control_panel_mode || "")
                    .trim()
                    .toLowerCase() === "aca"
                    ? "ok"
                    : "info"
                }
              >
                {installProfileQuery.data?.control_panel_mode || "auto"}
              </Badge>
              <Badge tone={installProfileQuery.data?.control_panel_config_ready ? "ok" : "warn"}>
                {installProfileQuery.data?.control_panel_config_ready ? "Ready" : "Needs setup"}
              </Badge>
              <button
                type="button"
                className="tcp-btn"
                onClick={() =>
                  installConfigQuery.refetch().then(() => toast("ok", "Install config refreshed."))
                }
              >
                <i data-lucide="refresh-cw"></i>
                Refresh
              </button>
              <button
                type="button"
                className="tcp-btn-primary"
                onClick={() => saveInstallConfigMutation.mutate()}
                disabled={saveInstallConfigMutation.isPending}
              >
                <i data-lucide="save"></i>
                Save config
              </button>
            </div>
          }
        >
          <div className="grid gap-4">
            <div className="grid gap-3 md:grid-cols-2">
              <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                <div className="font-medium">Startup profile</div>
                <div className="tcp-subtle mt-1 text-xs">
                  {installProfileQuery.data?.control_panel_mode_reason ||
                    "The control panel auto-detects its startup mode and can be overridden with TANDEM_CONTROL_PANEL_MODE."}
                </div>
                <div className="mt-3 grid gap-2 text-xs">
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Mode source</span>
                    <span>{installProfileQuery.data?.control_panel_mode_source || "detected"}</span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Integration detected</span>
                    <span>{installProfileQuery.data?.aca_integration ? "yes" : "no"}</span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Compact nav</span>
                    <span>
                      {installProfileQuery.data?.control_panel_compact_nav ? "on" : "off"}
                    </span>
                  </div>
                </div>
              </div>
              <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                <div className="font-medium">Hosted management</div>
                <div className="tcp-subtle mt-1 text-xs">
                  Detect whether this panel is running on a Tandem-managed hosted deployment so
                  hosted-only update and notification UX can stay gated.
                </div>
                <div className="mt-3 grid gap-2 text-xs">
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Managed hosted server</span>
                    <span>{installProfileQuery.data?.hosted_managed ? "yes" : "no"}</span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Provider</span>
                    <span>{installProfileQuery.data?.hosted_provider || "—"}</span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Deployment slug</span>
                    <span>{installProfileQuery.data?.hosted_deployment_slug || "—"}</span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Hosted auth</span>
                    <span>
                      {installProfileQuery.data?.hosted_auth_available
                        ? "connected"
                        : "not connected"}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Release</span>
                    <span>
                      {installProfileQuery.data?.hosted_release_version || "—"}
                      {installProfileQuery.data?.hosted_release_channel
                        ? ` · ${installProfileQuery.data.hosted_release_channel}`
                        : ""}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Update policy</span>
                    <span>{installProfileQuery.data?.hosted_update_policy || "—"}</span>
                  </div>
                </div>
                {installProfileQuery.data?.hosted_managed ? (
                  <div className="mt-3 rounded-xl border border-lime-500/20 bg-lime-500/10 px-3 py-2 text-xs text-lime-200">
                    Hosted-managed features can safely key off this signal instead of guessing from
                    hostname or environment.
                  </div>
                ) : null}
              </div>
              <div className="rounded-2xl border border-slate-700/60 bg-slate-950/25 p-4">
                <div className="font-medium">Config file</div>
                <div className="tcp-subtle mt-1 break-all text-xs">
                  {installProfileQuery.data?.control_panel_config_path ||
                    installConfigQuery.data?.path ||
                    "control-panel-config.json"}
                </div>
                <div className="mt-3 grid gap-2 text-xs">
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Ready</span>
                    <span>
                      {installProfileQuery.data?.control_panel_config_ready ? "yes" : "no"}
                    </span>
                  </div>
                  <div className="flex items-center justify-between gap-3">
                    <span className="tcp-subtle">Missing</span>
                    <span>
                      {Array.isArray(installProfileQuery.data?.control_panel_config_missing)
                        ? installProfileQuery.data?.control_panel_config_missing.join(", ") ||
                          "none"
                        : "unknown"}
                    </span>
                  </div>
                </div>
              </div>
            </div>

            <label className="grid gap-2">
              <span className="text-sm font-medium">Control panel config JSON</span>
              <textarea
                className="tcp-input min-h-[28rem] font-mono text-xs leading-5"
                value={installConfigText}
                onInput={(event) =>
                  setInstallConfigText((event.target as HTMLTextAreaElement).value)
                }
                spellCheck={false}
              />
            </label>

            {installConfigError ? (
              <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-sm text-rose-200">
                {installConfigError}
              </div>
            ) : null}

            <div className="tcp-subtle text-xs">
              This file holds non-secret install state: repo binding, provider defaults, task
              source, swarm policy, GitHub MCP preferences, and navigation defaults. Secrets should
              stay in `.env` or token files.
            </div>
          </div>
        </PanelCard>
      ) : null}

      {activeSection === "providers" ? (
        <PanelCard
          title="Provider defaults"
          subtitle="Provider catalog, model selection, and API key entry."
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone={String(providersConfig.data?.default || "").trim() ? "ok" : "warn"}>
                Default: {String(providersConfig.data?.default || "none")}
              </Badge>
              <Badge tone="info">{connectedProviderCount} connected</Badge>
              <button
                className="tcp-btn"
                onClick={() =>
                  refreshProviderStatus().then(() => toast("ok", "Provider status refreshed."))
                }
              >
                <i data-lucide="refresh-cw"></i>
                Refresh provider
              </button>
            </div>
          }
        >
          <div className="grid gap-3">
            <button
              type="button"
              className="tcp-list-item text-left"
              onClick={() => setProviderDefaultsOpen((prev) => !prev)}
              aria-expanded={providerDefaultsOpen}
            >
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="font-medium inline-flex items-center gap-2">
                    <i data-lucide={providerDefaultsOpen ? "chevron-down" : "chevron-right"}></i>
                    <span>
                      {providerDefaultsOpen ? "Hide provider catalog" : "Show provider catalog"}
                    </span>
                  </div>
                  <div className="tcp-subtle mt-1 text-xs">
                    {safeProviders.length} providers available for configuration. Expand to change
                    models and API keys.
                  </div>
                </div>
                <Badge tone="info">{connectedProviderCount} connected</Badge>
              </div>
            </button>

            <AnimatePresence initial={false}>
              {providerDefaultsOpen ? (
                <motion.div
                  className="grid gap-3"
                  initial={{ opacity: 0, y: -8 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -8 }}
                >
                  <div className="tcp-list-item grid gap-3">
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div>
                        <div className="font-medium">Custom OpenAI-compatible provider</div>
                        <div className="tcp-subtle mt-1 text-xs">
                          Add providers like MiniMax by ID, base URL, default model, and API key.
                        </div>
                      </div>
                      <Badge tone={safeCustomConfiguredProviders.length ? "ok" : "info"}>
                        {safeCustomConfiguredProviders.length} configured
                      </Badge>
                    </div>

                    <button
                      type="button"
                      className="tcp-list-item text-left"
                      onClick={() => setCustomProviderFormOpen((prev) => !prev)}
                      aria-expanded={customProviderFormOpen}
                    >
                      <div className="flex items-center justify-between gap-3">
                        <div>
                          <div className="font-medium inline-flex items-center gap-2">
                            <i
                              data-lucide={
                                customProviderFormOpen ? "chevron-down" : "chevron-right"
                              }
                            ></i>
                            <span>
                              {customProviderFormOpen
                                ? "Hide custom provider form"
                                : "Show custom provider form"}
                            </span>
                          </div>
                          <div className="tcp-subtle mt-1 text-xs">
                            Use this for OpenAI-compatible endpoints. Anthropic is handled by the
                            built-in provider row below.
                          </div>
                        </div>
                        <Badge tone="info">OpenAI-compatible only</Badge>
                      </div>
                    </button>

                    <AnimatePresence initial={false}>
                      {customProviderFormOpen ? (
                        <motion.div
                          className="grid gap-3"
                          initial={{ opacity: 0, y: -8 }}
                          animate={{ opacity: 1, y: 0 }}
                          exit={{ opacity: 0, y: -8 }}
                        >
                          <form
                            className="grid gap-3"
                            onSubmit={(event) => {
                              event.preventDefault();
                              saveCustomProviderMutation.mutate({
                                providerId: customProviderId,
                                url: customProviderUrl,
                                modelId: customProviderModel,
                                apiKey: customProviderApiKey,
                                makeDefault: customProviderMakeDefault,
                              });
                            }}
                          >
                            <div className="grid gap-3 md:grid-cols-2">
                              <div className="grid gap-2">
                                <label className="text-sm font-medium">Provider ID</label>
                                <input
                                  className="tcp-input"
                                  value={customProviderId}
                                  onInput={(event) =>
                                    setCustomProviderId((event.target as HTMLInputElement).value)
                                  }
                                  placeholder="custom"
                                />
                              </div>
                              <div className="grid gap-2">
                                <label className="text-sm font-medium">Default model</label>
                                <input
                                  className="tcp-input"
                                  value={customProviderModel}
                                  onInput={(event) =>
                                    setCustomProviderModel((event.target as HTMLInputElement).value)
                                  }
                                  placeholder="MiniMax-M2"
                                />
                              </div>
                            </div>
                            <div className="grid gap-2">
                              <label className="text-sm font-medium">Base URL</label>
                              <input
                                className="tcp-input"
                                value={customProviderUrl}
                                onInput={(event) =>
                                  setCustomProviderUrl((event.target as HTMLInputElement).value)
                                }
                                placeholder="https://api.minimax.io/v1"
                              />
                            </div>
                            <div className="grid gap-2">
                              <label className="text-sm font-medium">API key</label>
                              <input
                                className="tcp-input"
                                type="password"
                                value={customProviderApiKey}
                                onInput={(event) =>
                                  setCustomProviderApiKey((event.target as HTMLInputElement).value)
                                }
                                placeholder="Optional. Leave blank to keep the existing key."
                              />
                            </div>
                            <label className="inline-flex items-center gap-2 text-sm text-slate-200">
                              <input
                                type="checkbox"
                                className="accent-slate-400"
                                checked={customProviderMakeDefault}
                                onChange={(event) =>
                                  setCustomProviderMakeDefault(
                                    (event.target as HTMLInputElement).checked
                                  )
                                }
                              />
                              Make this the default provider
                            </label>
                            <div className="flex flex-wrap justify-end gap-2">
                              <button
                                className="tcp-btn-primary"
                                type="submit"
                                disabled={saveCustomProviderMutation.isPending}
                              >
                                <i data-lucide="plus"></i>
                                Save custom provider
                              </button>
                            </div>
                          </form>
                        </motion.div>
                      ) : null}
                    </AnimatePresence>

                    {safeCustomConfiguredProviders.length ? (
                      <div className="grid gap-2">
                        {safeCustomConfiguredProviders.map((provider) => (
                          <div
                            key={provider.id}
                            className="flex flex-wrap items-start justify-between gap-2 rounded-xl border border-slate-700/60 bg-slate-900/20 px-3 py-2"
                          >
                            <div className="min-w-0">
                              <div className="font-medium">{provider.id}</div>
                              <div className="tcp-subtle break-all text-xs">
                                {provider.url || "No URL configured"}
                              </div>
                              <div className="tcp-subtle text-xs">
                                Model: {provider.model || "not set"}
                              </div>
                            </div>
                            <div className="flex flex-wrap gap-2">
                              {provider.isDefault ? <Badge tone="ok">default</Badge> : null}
                              <button
                                type="button"
                                className="tcp-btn h-8 px-3 text-xs"
                                onClick={() => {
                                  setCustomProviderId(provider.id);
                                  setCustomProviderUrl(provider.url);
                                  setCustomProviderModel(provider.model);
                                  setCustomProviderMakeDefault(provider.isDefault);
                                  setCustomProviderFormOpen(true);
                                  setProviderDefaultsOpen(true);
                                }}
                              >
                                <i data-lucide="square-pen"></i>
                                Edit
                              </button>
                            </div>
                          </div>
                        ))}
                      </div>
                    ) : null}
                  </div>

                  {providersCatalog.isPending ? (
                    <div className="tcp-list-item grid gap-2">
                      <div className="font-medium">Loading provider catalog</div>
                      <div className="tcp-subtle text-xs">
                        Tandem is checking live provider models and auth state now.
                      </div>
                    </div>
                  ) : safeProviders.length ? (
                    safeProviders.map((provider: any) => {
                      const providerId = String(provider?.id || "");
                      const models = Object.keys(provider?.models || {});
                      const defaultModel = String(
                        providersConfig.data?.providers?.[providerId]?.default_model ||
                          models[0] ||
                          ""
                      );
                      const typedModel = String(
                        modelSearchByProvider[providerId] ?? defaultModel
                      ).trim();
                      const normalizedTyped = typedModel.toLowerCase();
                      const filteredModels = models
                        .filter((modelId) =>
                          normalizedTyped ? modelId.toLowerCase().includes(normalizedTyped) : true
                        )
                        .slice(0, 80);
                      const badge = providerCatalogBadge(provider, models.length);
                      const subtitle = providerCatalogSubtitle(provider, defaultModel);
                      const providerHint =
                        (providerHints as Record<string, any>)[providerId] || null;
                      const keyUrl = String(providerHint?.keyUrl || "").trim();
                      const providerAuth = providerAuthById[providerId] || {};
                      const currentDefaultProvider = String(providersConfig.data?.default || "")
                        .trim()
                        .toLowerCase();
                      const codexIsDefaultProvider =
                        providerId === OPENAI_CODEX_PROVIDER_ID &&
                        currentDefaultProvider === OPENAI_CODEX_PROVIDER_ID;
                      const authKind = String(
                        providerAuth?.auth_kind || providerAuth?.authKind || ""
                      )
                        .trim()
                        .toLowerCase();
                      const oauthStatus = String(providerAuth?.status || "")
                        .trim()
                        .toLowerCase();
                      const oauthEmail = String(providerAuth?.email || "").trim();
                      const oauthDisplayName = String(
                        providerAuth?.display_name || providerAuth?.displayName || ""
                      ).trim();
                      const oauthManagedBy = String(
                        providerAuth?.managed_by || providerAuth?.managedBy || ""
                      ).trim();
                      const oauthExpiresAtMs = Number(
                        providerAuth?.expires_at_ms || providerAuth?.expiresAtMs || 0
                      );
                      const localCodexSessionAvailable =
                        providerAuth?.local_session_available === true ||
                        providerAuth?.localSessionAvailable === true;
                      const oauthSessionId = String(
                        oauthSessionByProvider[providerId] || ""
                      ).trim();
                      const oauthPending =
                        !!oauthSessionId ||
                        (authorizeProviderOAuthMutation.isPending &&
                          String(authorizeProviderOAuthMutation.variables?.providerId || "")
                            .trim()
                            .toLowerCase() === providerId);
                      const oauthConnected =
                        authKind === "oauth" &&
                        providerAuth?.connected === true &&
                        oauthStatus !== "reauth_required";
                      const supportsOAuth = providerId === OPENAI_CODEX_PROVIDER_ID;
                      const canUseOAuthHere = !supportsOAuth || localEngine || hostedManaged;
                      const oauthBadge = oauthPending
                        ? { tone: "info" as const, text: "sign-in pending" }
                        : oauthConnected
                          ? { tone: "ok" as const, text: "account connected" }
                          : oauthStatus === "reauth_required"
                            ? { tone: "warn" as const, text: "reauth required" }
                            : { tone: "warn" as const, text: "not connected" };
                      const hostedCodexImportFlow =
                        hostedManaged && providerId === OPENAI_CODEX_PROVIDER_ID;

                      return (
                        <motion.details key={providerId} layout className="tcp-list-item">
                          <summary className="cursor-pointer list-none">
                            <div className="flex items-center justify-between gap-3">
                              <div>
                                <div className="font-medium">{providerId}</div>
                                <div className="tcp-subtle text-xs">{subtitle}</div>
                              </div>
                              <Badge tone={badge.tone}>{badge.text}</Badge>
                            </div>
                          </summary>
                          <div className="mt-3 grid gap-3">
                            {keyUrl && !supportsOAuth ? (
                              <div className="flex justify-end">
                                <a
                                  className="tcp-btn h-8 px-3 text-xs"
                                  href={keyUrl}
                                  target="_blank"
                                  rel="noreferrer"
                                >
                                  <i data-lucide="external-link"></i>
                                  Get API key
                                </a>
                              </div>
                            ) : null}
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
                              <div className="max-h-48 overflow-auto rounded-xl border border-slate-700/60 bg-slate-900/20 p-1">
                                {filteredModels.length ? (
                                  filteredModels.map((modelId) => (
                                    <button
                                      key={modelId}
                                      type="button"
                                      className={`block w-full rounded-lg px-2 py-1.5 text-left text-sm hover:bg-slate-700/30 ${
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
                                  <div className="tcp-subtle px-2 py-1 text-xs">
                                    {models.length
                                      ? "No matching models."
                                      : "No live catalog available. Type a model ID manually."}
                                  </div>
                                )}
                              </div>
                            </form>

                            {supportsOAuth ? (
                              <div className="grid gap-3 rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                                <div className="flex flex-wrap items-start justify-between gap-3">
                                  <div className="min-w-0">
                                    <div className="font-medium">
                                      {String(providerHint?.label || "Codex Account")}
                                    </div>
                                    <div className="tcp-subtle text-xs">
                                      {String(
                                        providerHint?.description ||
                                          "Use your ChatGPT/Codex subscription instead of a separate API key."
                                      )}
                                    </div>
                                  </div>
                                  <Badge tone={oauthBadge.tone}>{oauthBadge.text}</Badge>
                                </div>

                                <div className="grid gap-1 text-xs tcp-subtle">
                                  {hostedCodexImportFlow ? (
                                    <div className="rounded-xl border border-sky-700/50 bg-sky-950/20 px-3 py-2 text-sky-100">
                                      <div className="font-medium">
                                        Recommended for hosted servers
                                      </div>
                                      <div className="mt-1">
                                        Import a Codex <code>auth.json</code> from a signed-in
                                        machine. Browser OAuth on provisioned servers can stall
                                        after the consent screen, so the import path is the reliable
                                        v1 flow.
                                      </div>
                                    </div>
                                  ) : null}
                                  {oauthPending ? (
                                    <div>
                                      Pending browser sign-in is saved in this browser session, so
                                      you can refresh this page and Tandem will keep checking when
                                      you come back.
                                    </div>
                                  ) : null}
                                  {oauthConnected ? (
                                    <div>
                                      {oauthDisplayName || oauthEmail
                                        ? `Connected as ${oauthDisplayName || oauthEmail}.`
                                        : "Connected to a Codex account."}
                                    </div>
                                  ) : null}
                                  {oauthManagedBy ? (
                                    <div>
                                      Managed by{" "}
                                      {oauthManagedBy === "codex-cli"
                                        ? "the local Codex CLI session"
                                        : oauthManagedBy === "codex-upload"
                                          ? "an uploaded Codex auth.json"
                                          : "Tandem"}
                                      .
                                    </div>
                                  ) : null}
                                  {hostedCodexImportFlow &&
                                  oauthConnected &&
                                  oauthManagedBy === "codex-upload" ? (
                                    <div>
                                      This hosted server is currently using an imported Codex
                                      session stored on the VM. Import another{" "}
                                      <code>auth.json</code> any time to replace it.
                                    </div>
                                  ) : null}
                                  {oauthExpiresAtMs > 0 ? (
                                    <div>
                                      Session status refreshes through{" "}
                                      {new Date(oauthExpiresAtMs).toLocaleString()}.
                                    </div>
                                  ) : null}
                                  {providerId === OPENAI_CODEX_PROVIDER_ID &&
                                  oauthConnected &&
                                  !codexIsDefaultProvider ? (
                                    <div>
                                      Tandem is connected to Codex, but new runs are still using a
                                      different default provider.
                                    </div>
                                  ) : null}
                                  {canUseOAuthHere ? (
                                    <div>
                                      {hostedCodexImportFlow
                                        ? "This Tandem-hosted server can import a Codex auth.json from a signed-in machine and keep the session on the VM."
                                        : localCodexSessionAvailable
                                          ? "Local Codex CLI session detected on this machine."
                                          : "If the Codex CLI is already signed in on this machine, you can mirror that session here instead of starting a fresh browser login."}
                                    </div>
                                  ) : null}
                                  {!canUseOAuthHere ? (
                                    <div>
                                      Codex account sign-in is only enabled when this control panel
                                      is connected to a local engine or a Tandem-hosted managed
                                      server.
                                    </div>
                                  ) : null}
                                </div>

                                {hostedCodexImportFlow ? (
                                  <div className="grid gap-3">
                                    <input
                                      ref={codexAuthInputRef}
                                      type="file"
                                      accept=".json,application/json"
                                      className="hidden"
                                      onChange={(event) => {
                                        const file = event.target.files?.[0] || null;
                                        void importCodexAuthFile(providerId, file);
                                      }}
                                    />
                                    <textarea
                                      className="tcp-input min-h-40 resize-y rounded-xl p-3 font-mono text-xs leading-5"
                                      value={codexAuthJsonText}
                                      onChange={(event) => setCodexAuthJsonText(event.target.value)}
                                      placeholder={`Paste the contents of ~/.codex/auth.json here.\n\nTandem will store it on this server and reuse it for Codex sessions.`}
                                    />
                                    <div className="grid gap-1 text-xs tcp-subtle">
                                      <div>
                                        You can paste the JSON directly, or choose the file from a
                                        signed-in machine.
                                      </div>
                                      {codexAuthFileName ? (
                                        <div>Loaded file: {codexAuthFileName}</div>
                                      ) : null}
                                    </div>
                                    <div className="flex flex-wrap gap-2">
                                      <button
                                        type="button"
                                        className="tcp-btn"
                                        disabled={
                                          !canUseOAuthHere ||
                                          !codexAuthJsonText.trim() ||
                                          importCodexAuthJsonMutation.isPending ||
                                          disconnectProviderOAuthMutation.isPending
                                        }
                                        onClick={() =>
                                          importCodexAuthJsonMutation.mutate({
                                            providerId,
                                            authJson: codexAuthJsonText,
                                          })
                                        }
                                      >
                                        <i data-lucide="upload"></i>
                                        {oauthConnected
                                          ? "Replace hosted Codex session"
                                          : "Import pasted auth.json"}
                                      </button>
                                      <button
                                        type="button"
                                        className="tcp-btn"
                                        disabled={
                                          importCodexAuthJsonMutation.isPending ||
                                          disconnectProviderOAuthMutation.isPending
                                        }
                                        onClick={() => codexAuthInputRef.current?.click()}
                                      >
                                        <i data-lucide="file-up"></i>
                                        Choose auth.json file
                                      </button>
                                      {localCodexSessionAvailable ? (
                                        <button
                                          type="button"
                                          className="tcp-btn h-10 px-4 text-sm"
                                          disabled={
                                            !canUseOAuthHere ||
                                            authorizeProviderOAuthMutation.isPending ||
                                            useLocalCodexSessionMutation.isPending
                                          }
                                          onClick={() => {
                                            setOauthSessionByProvider((current) => {
                                              if (!current[providerId]) return current;
                                              const next = { ...current };
                                              delete next[providerId];
                                              return next;
                                            });
                                            useLocalCodexSessionMutation.mutate({
                                              providerId,
                                            });
                                          }}
                                        >
                                          <i data-lucide="link-2"></i>
                                          Use Local Codex Session
                                        </button>
                                      ) : null}
                                      {providerId === OPENAI_CODEX_PROVIDER_ID &&
                                      oauthConnected &&
                                      !codexIsDefaultProvider ? (
                                        <button
                                          type="button"
                                          className="tcp-btn h-10 px-4 text-sm"
                                          disabled={setDefaultsMutation.isPending}
                                          onClick={() =>
                                            setDefaultsMutation.mutate({
                                              providerId,
                                              modelId: defaultModel || "gpt-5.4",
                                            })
                                          }
                                        >
                                          <i data-lucide="sparkles"></i>
                                          Use for Tandem Runs
                                        </button>
                                      ) : null}
                                      <button
                                        type="button"
                                        className="tcp-btn h-10 px-4 text-sm"
                                        disabled={
                                          !oauthConnected ||
                                          disconnectProviderOAuthMutation.isPending ||
                                          importCodexAuthJsonMutation.isPending ||
                                          oauthPending
                                        }
                                        onClick={() =>
                                          disconnectProviderOAuthMutation.mutate({
                                            providerId,
                                          })
                                        }
                                      >
                                        <i data-lucide="unlink"></i>
                                        Disconnect
                                      </button>
                                    </div>
                                  </div>
                                ) : (
                                  <div className="flex flex-wrap gap-2">
                                    <button
                                      type="button"
                                      className="tcp-btn"
                                      disabled={
                                        !canUseOAuthHere ||
                                        oauthPending ||
                                        disconnectProviderOAuthMutation.isPending
                                      }
                                      onClick={() =>
                                        authorizeProviderOAuthMutation.mutate({
                                          providerId,
                                        })
                                      }
                                    >
                                      <i data-lucide="log-in"></i>
                                      {oauthConnected
                                        ? "Reconnect Codex Account"
                                        : "Connect Codex Account"}
                                    </button>
                                    {localEngine &&
                                    providerId === OPENAI_CODEX_PROVIDER_ID &&
                                    localCodexSessionAvailable ? (
                                      <button
                                        type="button"
                                        className="tcp-btn h-10 px-4 text-sm"
                                        disabled={
                                          !canUseOAuthHere ||
                                          authorizeProviderOAuthMutation.isPending ||
                                          useLocalCodexSessionMutation.isPending
                                        }
                                        onClick={() => {
                                          setOauthSessionByProvider((current) => {
                                            if (!current[providerId]) return current;
                                            const next = { ...current };
                                            delete next[providerId];
                                            return next;
                                          });
                                          useLocalCodexSessionMutation.mutate({
                                            providerId,
                                          });
                                        }}
                                      >
                                        <i data-lucide="link-2"></i>
                                        Use Local Codex Session
                                      </button>
                                    ) : null}
                                    {providerId === OPENAI_CODEX_PROVIDER_ID &&
                                    oauthConnected &&
                                    !codexIsDefaultProvider ? (
                                      <button
                                        type="button"
                                        className="tcp-btn h-10 px-4 text-sm"
                                        disabled={setDefaultsMutation.isPending}
                                        onClick={() =>
                                          setDefaultsMutation.mutate({
                                            providerId,
                                            modelId: defaultModel || "gpt-5.4",
                                          })
                                        }
                                      >
                                        <i data-lucide="sparkles"></i>
                                        Use for Tandem Runs
                                      </button>
                                    ) : null}
                                    <button
                                      type="button"
                                      className="tcp-btn h-10 px-4 text-sm"
                                      disabled={
                                        !oauthConnected ||
                                        disconnectProviderOAuthMutation.isPending ||
                                        oauthPending
                                      }
                                      onClick={() =>
                                        disconnectProviderOAuthMutation.mutate({
                                          providerId,
                                        })
                                      }
                                    >
                                      <i data-lucide="unlink"></i>
                                      Disconnect
                                    </button>
                                    {oauthPending ? (
                                      <button
                                        type="button"
                                        className="tcp-btn h-10 px-4 text-sm"
                                        onClick={() =>
                                          window.open(
                                            "https://chatgpt.com/codex",
                                            "_blank",
                                            "noopener,noreferrer"
                                          )
                                        }
                                      >
                                        <i data-lucide="external-link"></i>
                                        Open Codex
                                      </button>
                                    ) : null}
                                  </div>
                                )}
                              </div>
                            ) : (
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
                                  placeholder={String(
                                    providerHint?.placeholder || `Set ${providerId} API key`
                                  )}
                                />
                                <button className="tcp-btn" type="submit">
                                  <i data-lucide="save"></i>
                                  Save
                                </button>
                              </form>
                            )}
                          </div>
                        </motion.details>
                      );
                    })
                  ) : (
                    <EmptyState text="No provider catalog is available yet. You can still enter a model ID manually for custom providers." />
                  )}
                </motion.div>
              ) : null}
            </AnimatePresence>
          </div>
        </PanelCard>
      ) : null}
    </>
  );
}
