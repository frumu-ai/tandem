import { AnimatePresence, motion } from "motion/react";
import { ProviderModelSelector } from "../components/ProviderModelSelector";
import { McpToolAllowlistEditor } from "../components/McpToolAllowlistEditor";
import { Badge, EmptyState, PanelCard, StaggerGroup, Toolbar } from "../ui/index.tsx";
import {
  CHANNEL_NAMES,
  CHANNEL_TOOL_GROUPS,
  WORKFLOW_PLANNER_PSEUDO_TOOL,
  channelConfigHasSavedSettings,
  channelDraftMatchesConfig,
  channelExactMcpToolsForServer,
  channelToolEnabled,
  defaultChannelToolPreferences,
  formatChannelScopeLabel,
  nextChannelExactMcpPreferences,
  nextChannelMcpPreferences,
  nextChannelToolPreferences,
  normalizeChannelDraft,
  normalizeChannelToolPreferences,
  normalizeMcpNamespaceSegment,
  normalizeMcpTools,
  toolAllowedForSecurityProfile,
  toolEnabledForSecurityProfile,
  uniqueChannelValues,
  useSettingsPageController,
} from "./SettingsPageController";

type SettingsPageControllerState = ReturnType<typeof useSettingsPageController>;

type SettingsPageChannelsMcpSectionsProps = {
  controller: SettingsPageControllerState;
};

function mcpServerNeedsOAuthAction(server: any) {
  const authKind = String(server?.authKind || server?.auth_kind || "")
    .trim()
    .toLowerCase();
  if (authKind !== "oauth") return false;
  const challenge = server?.lastAuthChallenge || server?.last_auth_challenge || null;
  const challengeUrl = String(
    challenge?.authorization_url ||
      challenge?.authorizationUrl ||
      server?.authorizationUrl ||
      server?.authorization_url ||
      ""
  ).trim();
  const lastError = String(server?.lastError || server?.last_error || "").toLowerCase();
  return (
    !server?.connected ||
    !!challenge ||
    !!challengeUrl ||
    lastError.includes("authorization") ||
    lastError.includes("invalid_token") ||
    lastError.includes("access token")
  );
}

function compareMcpServersForDisplay(a: any, b: any) {
  const attentionDelta =
    Number(mcpServerNeedsOAuthAction(b)) - Number(mcpServerNeedsOAuthAction(a));
  if (attentionDelta) return attentionDelta;
  return String(a?.name || "").localeCompare(String(b?.name || ""));
}

export function SettingsPageChannelsMcpSections({
  controller,
}: SettingsPageChannelsMcpSectionsProps) {
  const {
    activeSection,
    channelDefaultModel,
    channelDrafts,
    channelProviderOptions,
    channelScopesQuery,
    channelToolPreferencesQuery,
    channelToolScopeOpen,
    channelToolScopeSelection,
    channelVerifyResult,
    channelsConfigQuery,
    channelsStatusQuery,
    connectedChannelCount,
    connectedMcpCount,
    deleteChannelMutation,
    invalidateChannels,
    invalidateMcp,
    mcpActionMutation,
    mcpServers,
    mcpToolIds,
    mcpToolPolicyMutation,
    openMcpModal,
    providers,
    saveChannelMutation,
    saveChannelToolPreferencesMutation,
    setActiveSection,
    setChannelDrafts,
    setChannelToolScopeOpen,
    setChannelToolScopeSelection,
    verifyChannelMutation,
  } = controller;
  const safeMcpServers = (Array.isArray(mcpServers) ? [...mcpServers] : []).sort(
    compareMcpServersForDisplay
  );

  return (
    <>
      {activeSection === "channels" ? (
        <PanelCard
          title="Channel connections"
          subtitle="Telegram, Discord, and Slack delivery setup and live listener status."
          actions={
            <Toolbar>
              <Badge tone={connectedChannelCount ? "ok" : "warn"}>
                {connectedChannelCount}/{CHANNEL_NAMES.length} connected
              </Badge>
              <button className="tcp-btn" onClick={() => void invalidateChannels()}>
                <i data-lucide="refresh-cw"></i>
                Refresh channels
              </button>
            </Toolbar>
          }
        >
          <div className="grid gap-3">
            {CHANNEL_NAMES.map((channel) => {
              const config = ((channelsConfigQuery.data as any)?.[channel] || {}) as any;
              const status = ((channelsStatusQuery.data as any)?.[channel] || {}) as any;
              const draft = channelDrafts[channel] || normalizeChannelDraft(channel, config);
              const verifyResult = channelVerifyResult[channel];
              const scopeOptions =
                ((channelScopesQuery.data as Record<string, any[]> | undefined) || {})[
                  channel
                ]?.slice() || [];
              const selectedScopeId = String(channelToolScopeSelection[channel] || "").trim();
              const selectedScope =
                scopeOptions.find((scope) => scope.scope_id === selectedScopeId) || null;
              const selectedScopeLabel = selectedScope
                ? formatChannelScopeLabel(selectedScope)
                : selectedScopeId || "Channel default";
              const scopeTargetLabel = selectedScopeId ? "scope" : "channel";
              const toolPrefs = normalizeChannelToolPreferences(
                (channelToolPreferencesQuery.data as Record<string, any> | undefined)?.[channel] ||
                  defaultChannelToolPreferences()
              );
              const knownExactMcpToolPrefixes = safeMcpServers.map(
                (server) => `mcp.${normalizeMcpNamespaceSegment(server.name)}.`
              );
              const knownExactMcpTools = new Set(
                toolPrefs.enabled_mcp_tools.filter((tool) =>
                  knownExactMcpToolPrefixes.some((prefix) => tool.startsWith(prefix))
                )
              );
              const enabledMcpServerPrefixes = new Set(
                toolPrefs.enabled_mcp_servers.map(
                  (server) => `mcp.${normalizeMcpNamespaceSegment(server)}.`
                )
              );
              const activeExactMcpTools = toolPrefs.enabled_mcp_tools.filter((tool) =>
                Array.from(enabledMcpServerPrefixes).some((prefix) => tool.startsWith(prefix))
              );
              const publicDemo = draft.securityProfile === "public_demo";
              const hasSavedConfig = channelConfigHasSavedSettings(channel, config);
              const channelSettingsDirty = !channelDraftMatchesConfig(channel, draft, config);

              return (
                <div key={channel} className="tcp-list-item grid gap-3">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <div className="font-semibold capitalize">{channel}</div>
                      <div className="tcp-subtle text-xs">
                        {channel === "telegram"
                          ? "Bot token, allowed users, style profile, and optional model override."
                          : channel === "discord"
                            ? "Bot token, allowed users, mention policy, guild targeting, and optional model override."
                            : "Bot token, allowed users, mention policy, default channel, and optional model override."}
                      </div>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Badge tone={status.connected ? "ok" : "warn"}>
                        {status.connected
                          ? "Connected"
                          : status.enabled
                            ? "Configured"
                            : "Disconnected"}
                      </Badge>
                      <Badge tone={config.has_token ? "info" : "warn"}>
                        {config.has_token ? "Token saved" : "No token"}
                      </Badge>
                    </div>
                  </div>

                  <div className="grid gap-3 md:grid-cols-2">
                    <input
                      className="tcp-input"
                      type="password"
                      placeholder={
                        config.has_token
                          ? String(config.token_masked || "****")
                          : `Paste ${channel} bot token`
                      }
                      value={draft.botToken}
                      onInput={(e) =>
                        setChannelDrafts((prev) => ({
                          ...prev,
                          [channel]: {
                            ...draft,
                            botToken: (e.target as HTMLInputElement).value,
                          },
                        }))
                      }
                    />
                    {config.has_token && !draft.botToken ? (
                      <div className="tcp-subtle text-xs">
                        Token is already stored. Enter a new token only if you want to replace it.
                      </div>
                    ) : null}
                    <input
                      className="tcp-input"
                      placeholder="Allowed users (comma separated)"
                      value={draft.allowedUsers}
                      onInput={(e) =>
                        setChannelDrafts((prev) => ({
                          ...prev,
                          [channel]: {
                            ...draft,
                            allowedUsers: (e.target as HTMLInputElement).value,
                          },
                        }))
                      }
                    />
                  </div>

                  <div className="grid gap-3 md:grid-cols-2">
                    <select
                      className="tcp-input"
                      value={draft.securityProfile}
                      onInput={(e) =>
                        setChannelDrafts((prev) => ({
                          ...prev,
                          [channel]: {
                            ...draft,
                            securityProfile: (e.target as HTMLSelectElement).value,
                          },
                        }))
                      }
                    >
                      <option value="operator">Operator</option>
                      <option value="trusted_team">Trusted team</option>
                      <option value="public_demo">Public demo</option>
                    </select>
                    {channel === "telegram" ? (
                      <input
                        className="tcp-input"
                        placeholder="Style profile"
                        value={draft.styleProfile}
                        onInput={(e) =>
                          setChannelDrafts((prev) => ({
                            ...prev,
                            [channel]: {
                              ...draft,
                              styleProfile: (e.target as HTMLInputElement).value,
                            },
                          }))
                        }
                      />
                    ) : null}
                    {channel === "discord" ? (
                      <input
                        className="tcp-input"
                        placeholder="Guild ID (optional)"
                        value={draft.guildId}
                        onInput={(e) =>
                          setChannelDrafts((prev) => ({
                            ...prev,
                            [channel]: {
                              ...draft,
                              guildId: (e.target as HTMLInputElement).value,
                            },
                          }))
                        }
                      />
                    ) : null}
                    {channel === "slack" ? (
                      <input
                        className="tcp-input"
                        placeholder="Default channel ID"
                        value={draft.channelId}
                        onInput={(e) =>
                          setChannelDrafts((prev) => ({
                            ...prev,
                            [channel]: {
                              ...draft,
                              channelId: (e.target as HTMLInputElement).value,
                            },
                          }))
                        }
                      />
                    ) : null}
                    <label className="inline-flex items-center gap-2 rounded-xl border border-slate-700/60 bg-slate-900/20 px-3 py-2 text-sm">
                      <input
                        type="checkbox"
                        checked={draft.mentionOnly}
                        onChange={(e) =>
                          setChannelDrafts((prev) => ({
                            ...prev,
                            [channel]: {
                              ...draft,
                              mentionOnly: e.target.checked,
                            },
                          }))
                        }
                      />
                      Mention only
                    </label>
                    <label className="flex items-center justify-between gap-3 rounded-xl border border-slate-700/60 bg-slate-900/20 px-3 py-2 text-sm">
                      <div className="flex flex-col">
                        <span>Strict KB grounding</span>
                        <span className="tcp-subtle text-[11px]">
                          Factual channel questions use selected MCP knowledge sources as the
                          grounding path.
                        </span>
                      </div>
                      <input
                        type="checkbox"
                        checked={draft.strictKbGrounding}
                        onChange={(e) =>
                          setChannelDrafts((prev) => ({
                            ...prev,
                            [channel]: {
                              ...draft,
                              strictKbGrounding: e.target.checked,
                            },
                          }))
                        }
                      />
                    </label>
                  </div>

                  <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
                    <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                      <div>
                        <div className="font-medium">Channel model override</div>
                        <div className="tcp-subtle text-xs">
                          Pick the provider and model this channel should use. Leave both blank to
                          inherit Tandem&apos;s global default.
                        </div>
                      </div>
                      <Badge tone={draft.modelProviderId && draft.modelId ? "ok" : "info"}>
                        {draft.modelProviderId && draft.modelId ? "Custom model" : "Global default"}
                      </Badge>
                    </div>
                    <ProviderModelSelector
                      providerLabel="Provider"
                      modelLabel="Model"
                      draft={{
                        provider: draft.modelProviderId,
                        model: draft.modelId,
                      }}
                      providers={channelProviderOptions}
                      onChange={({ provider, model }) =>
                        setChannelDrafts((prev) => ({
                          ...prev,
                          [channel]: {
                            ...draft,
                            modelProviderId: provider,
                            modelId: model,
                          },
                        }))
                      }
                      inheritLabel="Use global default"
                    />
                    <div className="mt-2 tcp-subtle text-xs">
                      {draft.modelProviderId && draft.modelId ? (
                        <span>
                          Selected model: <strong>{draft.modelProviderId}</strong> /{" "}
                          <strong>{draft.modelId}</strong>
                        </span>
                      ) : channelDefaultModel.provider && channelDefaultModel.model ? (
                        <span>
                          Inheriting Tandem default: <strong>{channelDefaultModel.provider}</strong>{" "}
                          / <strong>{channelDefaultModel.model}</strong>
                        </span>
                      ) : (
                        <span>No global default model is configured yet.</span>
                      )}
                    </div>
                  </div>

                  {draft.securityProfile === "public_demo" ? (
                    <div className="tcp-subtle text-xs">
                      Public demo mode blocks operator commands, file/workspace access, MCP access,
                      shell access, and tool-scope widening. Memory stays confined to this
                      channel&apos;s quarantined public namespace, and `/help` still advertises
                      restricted capabilities for security.
                    </div>
                  ) : null}

                  <div className="tcp-subtle text-xs">
                    Active sessions: {Number(status.active_sessions || 0)}
                    {status.last_error ? ` · Last error: ${status.last_error}` : ""}
                  </div>

                  {verifyResult?.hints?.length ? (
                    <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3 text-xs">
                      <div className="mb-1 font-medium">Verification hints</div>
                      <div className="grid gap-1">
                        {verifyResult.hints.map((hint: string, index: number) => (
                          <div key={`${channel}-hint-${index}`} className="tcp-subtle">
                            {hint}
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}

                  <div className="flex flex-wrap gap-2">
                    <button
                      className="tcp-btn-primary"
                      disabled={saveChannelMutation.isPending || !channelSettingsDirty}
                      onClick={() => saveChannelMutation.mutate(channel)}
                    >
                      <i data-lucide="save"></i>
                      Save
                    </button>
                    {channel === "discord" ? (
                      <button
                        className="tcp-btn"
                        disabled={verifyChannelMutation.isPending}
                        onClick={() => verifyChannelMutation.mutate("discord")}
                      >
                        <i data-lucide="shield-check"></i>
                        Verify
                      </button>
                    ) : null}
                    <button
                      className="tcp-btn-danger"
                      disabled={deleteChannelMutation.isPending || !hasSavedConfig}
                      onClick={() => deleteChannelMutation.mutate(channel)}
                    >
                      <i data-lucide="trash-2"></i>
                      Remove
                    </button>
                  </div>

                  <motion.div
                    layout
                    className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3"
                  >
                    <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
                      <div>
                        <div className="font-medium">Channel tool scope</div>
                        <div className="tcp-subtle text-xs">
                          Built-in tools and MCP servers available to {channel} sessions.
                        </div>
                        {toolPrefs.enabled_tools.some(
                          (tool) => tool !== WORKFLOW_PLANNER_PSEUDO_TOOL
                        ) ? (
                          <div className="mt-1 text-xs text-amber-300">
                            Explicit built-in allowlist is active for this {scopeTargetLabel}.
                          </div>
                        ) : null}
                        {publicDemo ? (
                          <div className="mt-1 text-xs text-slate-400">
                            Public demo profile can only expose web and quarantined public-memory
                            tools here. File, shell, MCP, and operator-facing tools stay disabled
                            even if saved in channel preferences.
                          </div>
                        ) : null}
                      </div>
                      <div className="flex flex-wrap items-center gap-2">
                        <button
                          className="tcp-btn"
                          disabled={saveChannelToolPreferencesMutation.isPending}
                          onClick={() =>
                            saveChannelToolPreferencesMutation.mutate({
                              channel,
                              scopeId: selectedScopeId || null,
                              payload: { reset: true },
                            })
                          }
                        >
                          Reset scope
                        </button>
                        <button
                          className="tcp-btn"
                          aria-expanded={!!channelToolScopeOpen[channel]}
                          onClick={() =>
                            setChannelToolScopeOpen((prev) => ({
                              ...prev,
                              [channel]: !prev[channel],
                            }))
                          }
                        >
                          <span>{channelToolScopeOpen[channel] ? "Hide" : "Show"}</span>
                          <i
                            data-lucide="chevron-down"
                            className={`h-4 w-4 transition-transform duration-200 ${
                              channelToolScopeOpen[channel] ? "rotate-180" : ""
                            }`}
                          ></i>
                        </button>
                      </div>
                    </div>

                    <div className="mb-3 grid gap-3 md:grid-cols-[minmax(0,1fr)_320px] md:items-end">
                      <div className="grid gap-1">
                        <div className="tcp-subtle text-xs">
                          {selectedScopeId
                            ? `Editing ${selectedScopeLabel}. Saving here stores a scope-specific override on top of the ${channel} default.`
                            : `Editing the ${channel} default. Pick a conversation scope to override one specific ${channel} thread, room, or chat.`}
                        </div>
                        <div className="tcp-subtle text-[11px]">
                          {scopeOptions.length
                            ? `${scopeOptions.length} known scope${
                                scopeOptions.length === 1 ? "" : "s"
                              } discovered from channel sessions.`
                            : "No scoped conversations discovered yet."}
                        </div>
                      </div>
                      <label className="grid gap-1">
                        <span className="tcp-subtle text-[11px] uppercase tracking-[0.24em]">
                          Conversation scope
                        </span>
                        <select
                          className="tcp-input"
                          value={selectedScopeId}
                          onChange={(e) =>
                            setChannelToolScopeSelection((prev) => ({
                              ...prev,
                              [channel]: (e.target as HTMLSelectElement).value,
                            }))
                          }
                        >
                          <option value="">Channel default</option>
                          {selectedScopeId &&
                          !scopeOptions.some((scope) => scope.scope_id === selectedScopeId) ? (
                            <option value={selectedScopeId}>{selectedScopeLabel}</option>
                          ) : null}
                          {scopeOptions.map((scope) => (
                            <option key={scope.scope_id} value={scope.scope_id}>
                              {formatChannelScopeLabel(scope)}
                            </option>
                          ))}
                        </select>
                      </label>
                    </div>

                    <div className="tcp-subtle text-xs">
                      {toolPrefs.enabled_mcp_servers.length
                        ? `${toolPrefs.enabled_mcp_servers.length} MCP server${
                            toolPrefs.enabled_mcp_servers.length === 1 ? "" : "s"
                          } enabled for this ${scopeTargetLabel}.`
                        : publicDemo
                          ? "MCP servers stay disabled in public demo mode."
                          : `No MCP servers enabled for this ${scopeTargetLabel}.`}
                      {activeExactMcpTools.length
                        ? ` ${activeExactMcpTools.length} exact MCP tool${
                            activeExactMcpTools.length === 1 ? "" : "s"
                          } also selected.`
                        : ""}
                      {` ${
                        toolAllowedForSecurityProfile(
                          draft.securityProfile,
                          WORKFLOW_PLANNER_PSEUDO_TOOL
                        )
                          ? channelToolEnabled(toolPrefs, WORKFLOW_PLANNER_PSEUDO_TOOL)
                            ? "Workflow drafts from chat are enabled."
                            : "Workflow drafts from chat are disabled."
                          : "Workflow drafts stay disabled in public demo mode."
                      }`}
                    </div>

                    <AnimatePresence initial={false}>
                      {channelToolScopeOpen[channel] ? (
                        <motion.div
                          key={`${channel}-tool-scope-body`}
                          initial={{ opacity: 0, height: 0, y: -6 }}
                          animate={{ opacity: 1, height: "auto", y: 0 }}
                          exit={{ opacity: 0, height: 0, y: -6 }}
                          transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
                          className="overflow-hidden"
                        >
                          <div className="grid gap-3 pt-3">
                            <div className="grid gap-2">
                              <div className="tcp-subtle text-[11px] uppercase tracking-[0.24em]">
                                Workflow planning
                              </div>
                              <label className="flex items-center justify-between rounded-xl border border-slate-700/60 bg-slate-950/30 px-3 py-2 text-sm">
                                <div className="flex flex-col">
                                  <span className="font-mono text-xs">
                                    Allow workflow drafts from chat
                                  </span>
                                  <span className="tcp-subtle text-[11px]">
                                    Stores the `tandem.workflow_planner` pseudo-tool in this channel
                                    scope without changing the normal tool allowlist.
                                  </span>
                                </div>
                                <input
                                  type="checkbox"
                                  checked={channelToolEnabled(
                                    toolPrefs,
                                    WORKFLOW_PLANNER_PSEUDO_TOOL
                                  )}
                                  disabled={
                                    saveChannelToolPreferencesMutation.isPending ||
                                    !toolAllowedForSecurityProfile(
                                      draft.securityProfile,
                                      WORKFLOW_PLANNER_PSEUDO_TOOL
                                    )
                                  }
                                  onChange={(e) =>
                                    saveChannelToolPreferencesMutation.mutate({
                                      channel,
                                      scopeId: selectedScopeId || null,
                                      payload: nextChannelToolPreferences(
                                        toolPrefs,
                                        WORKFLOW_PLANNER_PSEUDO_TOOL,
                                        e.currentTarget.checked
                                      ),
                                    })
                                  }
                                />
                              </label>
                            </div>

                            {CHANNEL_TOOL_GROUPS.map((group) => (
                              <div key={`${channel}-${group.label}`} className="grid gap-2">
                                <div className="tcp-subtle text-[11px] uppercase tracking-[0.24em]">
                                  {group.label}
                                </div>
                                <div className="grid gap-2 md:grid-cols-2">
                                  {group.tools.map((tool) => {
                                    const allowed = toolAllowedForSecurityProfile(
                                      draft.securityProfile,
                                      tool
                                    );
                                    const enabled = toolEnabledForSecurityProfile(
                                      toolPrefs,
                                      tool,
                                      draft.securityProfile
                                    );
                                    return (
                                      <label
                                        key={`${channel}-tool-${tool}`}
                                        className="flex items-center justify-between rounded-xl border border-slate-700/60 bg-slate-950/30 px-3 py-2 text-sm"
                                      >
                                        <div className="flex flex-col">
                                          <span className="font-mono text-xs">{tool}</span>
                                          {!allowed ? (
                                            <span className="tcp-subtle text-[11px]">
                                              Disabled by security profile
                                            </span>
                                          ) : null}
                                        </div>
                                        <input
                                          type="checkbox"
                                          checked={enabled}
                                          disabled={
                                            saveChannelToolPreferencesMutation.isPending || !allowed
                                          }
                                          onChange={(e) =>
                                            saveChannelToolPreferencesMutation.mutate({
                                              channel,
                                              scopeId: selectedScopeId || null,
                                              payload: nextChannelToolPreferences(
                                                toolPrefs,
                                                tool,
                                                e.currentTarget.checked
                                              ),
                                            })
                                          }
                                        />
                                      </label>
                                    );
                                  })}
                                </div>
                              </div>
                            ))}

                            <div className="grid gap-2">
                              <div className="tcp-subtle text-[11px] uppercase tracking-[0.24em]">
                                MCP servers
                              </div>
                              {safeMcpServers.length ? (
                                <div className="grid gap-2 md:grid-cols-2">
                                  {safeMcpServers.map((server) => {
                                    const enabled =
                                      !publicDemo &&
                                      toolPrefs.enabled_mcp_servers.includes(server.name);
                                    return (
                                      <label
                                        key={`${channel}-mcp-${server.name}`}
                                        className="flex items-center justify-between rounded-xl border border-slate-700/60 bg-slate-950/30 px-3 py-2 text-sm"
                                      >
                                        <div className="flex flex-col">
                                          <span className="font-mono text-xs">{server.name}</span>
                                          {publicDemo ? (
                                            <span className="tcp-subtle text-[11px]">
                                              Disabled by security profile
                                            </span>
                                          ) : null}
                                        </div>
                                        <input
                                          type="checkbox"
                                          checked={enabled}
                                          disabled={
                                            saveChannelToolPreferencesMutation.isPending ||
                                            publicDemo
                                          }
                                          onChange={(e) =>
                                            saveChannelToolPreferencesMutation.mutate({
                                              channel,
                                              scopeId: selectedScopeId || null,
                                              payload: nextChannelMcpPreferences(
                                                toolPrefs,
                                                server.name,
                                                e.currentTarget.checked
                                              ),
                                            })
                                          }
                                        />
                                      </label>
                                    );
                                  })}
                                </div>
                              ) : (
                                <div className="tcp-subtle text-xs">
                                  {publicDemo
                                    ? "MCP servers stay disabled in public demo mode."
                                    : "No MCP servers configured yet."}
                                </div>
                              )}
                            </div>

                            <div className="grid gap-2">
                              <div className="tcp-subtle text-[11px] uppercase tracking-[0.24em]">
                                Exact MCP tools
                              </div>
                              <div className="tcp-subtle text-xs">
                                Choose exact tool names for this {scopeTargetLabel}. Exact tools
                                only apply when their MCP server is enabled above.
                              </div>
                              {safeMcpServers.length ? (
                                <div className="grid gap-3">
                                  {safeMcpServers.map((server) => {
                                    const discoveredTools = normalizeMcpTools(
                                      Array.isArray(server.toolCache) ? server.toolCache : []
                                    );
                                    const serverEnabled =
                                      !publicDemo &&
                                      toolPrefs.enabled_mcp_servers.includes(server.name);
                                    const selectedExactTools = channelExactMcpToolsForServer(
                                      toolPrefs,
                                      server.name,
                                      discoveredTools
                                    );
                                    return (
                                      <McpToolAllowlistEditor
                                        key={`${channel}-exact-mcp-${server.name}`}
                                        title={server.name}
                                        subtitle={
                                          !serverEnabled
                                            ? "Server is disabled for this scope. Exact tool selections are inactive until you enable the server above."
                                            : server.connected
                                              ? server.enabled
                                                ? "Connected and enabled globally. Pick the exact tools this scope can use."
                                                : "Connected, but disabled globally. Exact selections are saved here and will apply if the server is enabled."
                                              : "This server is disconnected. Exact selections are saved here and will apply when it reconnects."
                                        }
                                        discoveredTools={discoveredTools}
                                        value={serverEnabled ? selectedExactTools : []}
                                        disabled={
                                          saveChannelToolPreferencesMutation.isPending ||
                                          publicDemo ||
                                          !serverEnabled
                                        }
                                        collapsible
                                        defaultCollapsed
                                        emptyText="No MCP tools have been discovered for this server yet."
                                        onChange={(next) =>
                                          saveChannelToolPreferencesMutation.mutate({
                                            channel,
                                            scopeId: selectedScopeId || null,
                                            payload: nextChannelExactMcpPreferences(
                                              toolPrefs,
                                              server.name,
                                              discoveredTools,
                                              next
                                            ),
                                          })
                                        }
                                      />
                                    );
                                  })}
                                  {toolPrefs.enabled_mcp_tools.filter(
                                    (tool) => !knownExactMcpTools.has(tool)
                                  ).length ? (
                                    <McpToolAllowlistEditor
                                      title="Saved exact tools not currently matched"
                                      subtitle="These exact MCP tools are still stored for this scope, but no discovered server is currently exposing them."
                                      discoveredTools={[]}
                                      value={toolPrefs.enabled_mcp_tools.filter(
                                        (tool) => !knownExactMcpTools.has(tool)
                                      )}
                                      disabled={
                                        saveChannelToolPreferencesMutation.isPending || publicDemo
                                      }
                                      emptyText="All saved exact MCP tools currently match a discovered server."
                                      onChange={(next) =>
                                        saveChannelToolPreferencesMutation.mutate({
                                          channel,
                                          scopeId: selectedScopeId || null,
                                          payload: {
                                            ...toolPrefs,
                                            enabled_mcp_tools: uniqueChannelValues([
                                              ...toolPrefs.enabled_mcp_tools.filter((tool) =>
                                                knownExactMcpTools.has(tool)
                                              ),
                                              ...(next === null ? [] : next),
                                            ]),
                                          },
                                        })
                                      }
                                    />
                                  ) : null}
                                </div>
                              ) : (
                                <div className="tcp-subtle text-xs">
                                  {publicDemo
                                    ? "Exact MCP tools stay disabled in public demo mode."
                                    : "No MCP servers configured yet."}
                                </div>
                              )}
                            </div>
                          </div>
                        </motion.div>
                      ) : null}
                    </AnimatePresence>
                  </motion.div>
                </div>
              );
            })}
          </div>
        </PanelCard>
      ) : null}

      {activeSection === "mcp" ? (
        <PanelCard
          title="MCP connections"
          subtitle="Configured MCP servers, connection state, and discovered tool coverage. Per-channel exact tool scopes live under Channels."
          actions={
            <div className="flex flex-wrap items-center justify-end gap-2">
              <Badge tone={connectedMcpCount ? "ok" : "warn"}>
                {connectedMcpCount}/{safeMcpServers.length} connected
              </Badge>
              <Badge tone="info">{mcpToolIds.length} tools</Badge>
              <button className="tcp-btn" onClick={() => setActiveSection("channels")}>
                Channel scopes
              </button>
              <button className="tcp-btn-primary" onClick={() => openMcpModal()}>
                <i data-lucide="plus"></i>
                Add MCP server
              </button>
              <button className="tcp-btn" onClick={() => void invalidateMcp()}>
                <i data-lucide="refresh-cw"></i>
                Reload
              </button>
            </div>
          }
        >
          <div className="grid gap-3">
            {safeMcpServers.length ? (
              safeMcpServers.map((server) => {
                const headerKeys = Object.keys(server.headers || {}).filter(Boolean);
                const toolCount = Array.isArray(server.toolCache) ? server.toolCache.length : 0;
                return (
                  <div key={server.name} className="tcp-list-item grid gap-2">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div>
                        <div className="font-semibold">{server.name}</div>
                        <div className="tcp-subtle text-sm">
                          {server.transport || "No transport set"}
                        </div>
                      </div>
                      <div className="flex flex-wrap gap-2">
                        <Badge tone={server.connected ? "ok" : "warn"}>
                          {server.connected ? "Connected" : "Disconnected"}
                        </Badge>
                        <Badge tone={server.enabled ? "info" : "warn"}>
                          {server.enabled ? "Enabled" : "Disabled"}
                        </Badge>
                        {String(server.authKind || "")
                          .trim()
                          .toLowerCase() === "oauth" ? (
                          <Badge tone="info">OAuth</Badge>
                        ) : null}
                        <Badge tone="info">{toolCount} tools</Badge>
                      </div>
                    </div>
                    {server.lastError ? (
                      <div className="rounded-xl border border-rose-700/60 bg-rose-950/20 px-2 py-1 text-xs text-rose-300">
                        {server.lastError}
                      </div>
                    ) : null}
                    {server.lastAuthChallenge ? (
                      <div className="rounded-xl border border-amber-700/60 bg-amber-950/20 px-3 py-2 text-xs text-amber-100">
                        <div className="font-medium">OAuth authorization pending</div>
                        <div className="tcp-subtle mt-1">
                          {String(server.lastAuthChallenge.message || "").trim() ||
                            "Open the authorization URL to finish connecting this MCP server."}
                        </div>
                        <div className="tcp-subtle mt-1">
                          Tandem will keep checking for completion automatically while this page is
                          open.
                        </div>
                        {String(
                          server.lastAuthChallenge.authorization_url ||
                            server.lastAuthChallenge.authorizationUrl ||
                            server.authorizationUrl ||
                            ""
                        ).trim() ? (
                          <div className="mt-2 flex flex-wrap gap-2">
                            <a
                              className="tcp-btn inline-flex h-8 px-3 text-xs"
                              href={String(
                                server.lastAuthChallenge.authorization_url ||
                                  server.lastAuthChallenge.authorizationUrl ||
                                  server.authorizationUrl ||
                                  ""
                              ).trim()}
                              target="_blank"
                              rel="noreferrer"
                            >
                              Open sign-in
                            </a>
                            <button
                              type="button"
                              className="tcp-btn inline-flex h-8 px-3 text-xs"
                              disabled={mcpActionMutation.isPending}
                              onClick={() =>
                                mcpActionMutation.mutate({
                                  action: "authenticate",
                                  server,
                                })
                              }
                            >
                              Mark sign-in complete
                            </button>
                          </div>
                        ) : null}
                      </div>
                    ) : null}
                    <div className="tcp-subtle text-xs">
                      {headerKeys.length
                        ? `Auth headers: ${headerKeys.join(", ")}`
                        : "No stored auth headers."}
                    </div>
                    <McpToolAllowlistEditor
                      title="Tool access"
                      subtitle="Expand to choose exact tools. Leave all discovered tools selected to expose the full MCP server."
                      discoveredTools={Array.isArray(server.toolCache) ? server.toolCache : []}
                      value={server.allowedTools}
                      disabled={mcpToolPolicyMutation.isPending}
                      collapsible
                      defaultCollapsed
                      onChange={(next) =>
                        mcpToolPolicyMutation.mutate({
                          serverName: server.name,
                          allowedTools: next,
                        })
                      }
                    />
                    <div className="flex flex-wrap gap-2">
                      <button className="tcp-btn" onClick={() => openMcpModal(server)}>
                        Edit
                      </button>
                      <button
                        className="tcp-btn"
                        disabled={mcpActionMutation.isPending}
                        onClick={() =>
                          mcpActionMutation.mutate({
                            action: server.connected ? "disconnect" : "connect",
                            server,
                          })
                        }
                      >
                        {server.connected ? "Disconnect" : "Connect"}
                      </button>
                      <button
                        className="tcp-btn"
                        disabled={mcpActionMutation.isPending}
                        onClick={() => mcpActionMutation.mutate({ action: "refresh", server })}
                      >
                        Refresh
                      </button>
                      <button
                        className="tcp-btn"
                        disabled={mcpActionMutation.isPending}
                        onClick={() =>
                          mcpActionMutation.mutate({ action: "toggle-enabled", server })
                        }
                      >
                        {server.enabled ? "Disable" : "Enable"}
                      </button>
                      <button
                        className="tcp-btn-danger"
                        disabled={mcpActionMutation.isPending}
                        onClick={() => mcpActionMutation.mutate({ action: "delete", server })}
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                );
              })
            ) : (
              <div className="grid gap-3">
                <EmptyState text="No MCP servers configured." />
                <div className="flex justify-start">
                  <button className="tcp-btn-primary" onClick={() => openMcpModal()}>
                    <i data-lucide="plus"></i>
                    Add MCP server
                  </button>
                </div>
              </div>
            )}

            <div className="rounded-xl border border-slate-700/60 bg-slate-900/20 p-3">
              <div className="mb-2 font-medium">Discovered tools</div>
              <pre className="tcp-code max-h-56 overflow-auto whitespace-pre-wrap break-words">
                {mcpToolIds.length
                  ? mcpToolIds.slice(0, 250).join("\n")
                  : "No MCP tools discovered yet. Connect a server first."}
              </pre>
            </div>
          </div>
        </PanelCard>
      ) : null}
    </>
  );
}
