import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, CheckCircle2, Link2, RefreshCw, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import {
  deleteChannelConnectionToken,
  disableChannelConnection,
  getChannelConnections,
  getChannelToolPreferences,
  mcpListServers,
  onSidecarEventV2,
  setChannelConnection,
  setChannelToolPreferences,
  verifyChannelConnection,
  type ChannelConnectionInput,
  type ChannelConnectionsView,
  type ChannelVerifyResult,
  type ChannelName,
  type ChannelToolPreferencesView,
  type McpServerRecord,
  type StreamEventEnvelopeV2,
} from "@/lib/tauri";

type ChannelDraft = {
  token: string;
  allowedUsers: string;
  mentionOnly: boolean;
  guildId: string;
  channelId: string;
  securityProfile: string;
};

type ChannelDrafts = Record<ChannelName, ChannelDraft>;
type VerifyTone = "ok" | "warn" | "error";
type ChannelVerifyFeedback = {
  tone: VerifyTone;
  text: string;
};

const CHANNELS: ChannelName[] = ["telegram", "discord", "slack"];
const BUILTIN_TOOL_GROUPS = [
  { label: "File", tools: ["read", "glob", "ls", "list", "grep", "codesearch", "search"] },
  { label: "Web", tools: ["websearch", "webfetch", "webfetch_html"] },
  { label: "Terminal", tools: ["bash", "write", "edit", "apply_patch"] },
  { label: "Memory", tools: ["memory_search", "memory_store", "memory_list"] },
  { label: "Other", tools: ["skill", "task", "question", "pack_builder"] },
] as const;
const PUBLIC_DEMO_ALLOWED_TOOLS = [
  "websearch",
  "webfetch",
  "webfetch_html",
  "memory_search",
  "memory_store",
  "memory_list",
] as const;

function defaultToolPreferences(): ChannelToolPreferencesView {
  return {
    enabled_tools: [],
    disabled_tools: [],
    enabled_mcp_servers: [],
  };
}

function unique(values: string[]): string[] {
  return Array.from(new Set(values.map((value) => value.trim()).filter(Boolean)));
}

function toolIsEnabled(prefs: ChannelToolPreferencesView, tool: string): boolean {
  if (prefs.disabled_tools.includes(tool)) return false;
  return prefs.enabled_tools.length === 0 || prefs.enabled_tools.includes(tool);
}

function nextToolPreferences(
  prefs: ChannelToolPreferencesView,
  tool: string,
  enabled: boolean
): ChannelToolPreferencesView {
  const disabled = prefs.disabled_tools.filter((entry) => entry !== tool);
  const explicitEnabled = prefs.enabled_tools.length > 0 ? [...prefs.enabled_tools] : [];

  if (enabled) {
    return {
      ...prefs,
      disabled_tools: disabled,
      enabled_tools:
        explicitEnabled.length > 0 ? unique([...explicitEnabled, tool]) : explicitEnabled,
    };
  }

  return {
    ...prefs,
    disabled_tools: unique([...disabled, tool]),
    enabled_tools:
      explicitEnabled.length > 0
        ? explicitEnabled.filter((entry) => entry !== tool)
        : explicitEnabled,
  };
}

function nextMcpServerPreferences(
  prefs: ChannelToolPreferencesView,
  server: string,
  enabled: boolean
): ChannelToolPreferencesView {
  const servers = prefs.enabled_mcp_servers.filter((entry) => entry !== server);
  return {
    ...prefs,
    enabled_mcp_servers: enabled ? unique([...servers, server]) : servers,
  };
}

function toolAllowedForSecurityProfile(securityProfile: string, tool: string): boolean {
  if (securityProfile !== "public_demo") return true;
  return PUBLIC_DEMO_ALLOWED_TOOLS.includes(tool as (typeof PUBLIC_DEMO_ALLOWED_TOOLS)[number]);
}

function toolEnabledForSecurityProfile(
  prefs: ChannelToolPreferencesView,
  tool: string,
  securityProfile: string
): boolean {
  return toolAllowedForSecurityProfile(securityProfile, tool) && toolIsEnabled(prefs, tool);
}

function toCsv(users: string[]): string {
  return users.join(", ");
}

function parseUsersCsv(raw: string): string[] {
  const users = raw
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
  return users.length > 0 ? users : ["*"];
}

function draftFromConnections(connections: ChannelConnectionsView): ChannelDrafts {
  return {
    telegram: {
      token: "",
      allowedUsers: toCsv(connections.telegram.config.allowed_users),
      mentionOnly: !!connections.telegram.config.mention_only,
      guildId: "",
      channelId: "",
      securityProfile: connections.telegram.config.security_profile ?? "operator",
    },
    discord: {
      token: "",
      allowedUsers: toCsv(connections.discord.config.allowed_users),
      mentionOnly: connections.discord.config.mention_only ?? true,
      guildId: connections.discord.config.guild_id ?? "",
      channelId: "",
      securityProfile: connections.discord.config.security_profile ?? "operator",
    },
    slack: {
      token: "",
      allowedUsers: toCsv(connections.slack.config.allowed_users),
      mentionOnly: false,
      guildId: "",
      channelId: connections.slack.config.channel_id ?? "",
      securityProfile: connections.slack.config.security_profile ?? "operator",
    },
  };
}

function defaultDrafts(): ChannelDrafts {
  return {
    telegram: {
      token: "",
      allowedUsers: "*",
      mentionOnly: false,
      guildId: "",
      channelId: "",
      securityProfile: "operator",
    },
    discord: {
      token: "",
      allowedUsers: "*",
      mentionOnly: true,
      guildId: "",
      channelId: "",
      securityProfile: "operator",
    },
    slack: {
      token: "",
      allowedUsers: "*",
      mentionOnly: false,
      guildId: "",
      channelId: "",
      securityProfile: "operator",
    },
  };
}

function statusTone(
  connected: boolean,
  enabled: boolean
): {
  dot: string;
  text: string;
  cardBorder: string;
} {
  if (connected) {
    return {
      dot: "bg-success",
      text: "text-success",
      cardBorder: "border-success/25",
    };
  }
  if (enabled) {
    return {
      dot: "bg-warning",
      text: "text-warning",
      cardBorder: "border-warning/25",
    };
  }
  return {
    dot: "bg-text-subtle",
    text: "text-text-subtle",
    cardBorder: "border-border",
  };
}

export function ConnectionsSettings() {
  const { t } = useTranslation(["settings", "common"]);
  const [connections, setConnections] = useState<ChannelConnectionsView | null>(null);
  const [drafts, setDrafts] = useState<ChannelDrafts>(defaultDrafts);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [savingChannel, setSavingChannel] = useState<ChannelName | null>(null);
  const [verifyingChannel, setVerifyingChannel] = useState<ChannelName | null>(null);
  const [verifyFeedback, setVerifyFeedback] = useState<
    Partial<Record<ChannelName, ChannelVerifyFeedback>>
  >({});
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [toolPreferences, setToolPreferences] = useState<
    Record<ChannelName, ChannelToolPreferencesView>
  >({
    telegram: defaultToolPreferences(),
    discord: defaultToolPreferences(),
    slack: defaultToolPreferences(),
  });
  const [mcpServers, setMcpServers] = useState<string[]>([]);
  const [scopeAction, setScopeAction] = useState<string | null>(null);

  const applyConnections = useCallback((next: ChannelConnectionsView) => {
    setConnections(next);
    setDrafts(draftFromConnections(next));
  }, []);

  const refresh = useCallback(
    async (showSpinner = false) => {
      if (showSpinner) setLoading(true);
      try {
        const [next, prefsEntries, serverRecords] = await Promise.all([
          getChannelConnections(),
          Promise.all(
            CHANNELS.map(async (channel) => {
              const prefs = await getChannelToolPreferences(channel).catch(() =>
                defaultToolPreferences()
              );
              return [channel, prefs] as const;
            })
          ),
          mcpListServers().catch(() => [] as McpServerRecord[]),
        ]);
        applyConnections(next);
        setToolPreferences({
          telegram:
            prefsEntries.find(([channel]) => channel === "telegram")?.[1] ??
            defaultToolPreferences(),
          discord:
            prefsEntries.find(([channel]) => channel === "discord")?.[1] ??
            defaultToolPreferences(),
          slack:
            prefsEntries.find(([channel]) => channel === "slack")?.[1] ?? defaultToolPreferences(),
        });
        setMcpServers(
          serverRecords
            .map((server) => String(server.name || "").trim())
            .filter(Boolean)
            .sort((a, b) => a.localeCompare(b))
        );
        setError(null);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load channel connections");
      } finally {
        if (showSpinner) setLoading(false);
      }
    },
    [applyConnections]
  );

  useEffect(() => {
    void refresh(true);
  }, [refresh]);

  useEffect(() => {
    const poll = globalThis.setInterval(() => {
      void refresh(false);
    }, 5000);

    let unlisten: (() => void) | null = null;
    const setup = async () => {
      unlisten = await onSidecarEventV2((envelope: StreamEventEnvelopeV2) => {
        const payload = envelope?.payload;
        if (!payload || payload.type !== "raw") return;
        if (!payload.event_type.startsWith("channel.")) return;
        void refresh(false);
      });
    };
    void setup();

    return () => {
      globalThis.clearInterval(poll);
      if (unlisten) unlisten();
    };
  }, [refresh]);

  const isBusy = useMemo(
    () =>
      savingChannel !== null ||
      verifyingChannel !== null ||
      busyAction !== null ||
      scopeAction !== null,
    [savingChannel, verifyingChannel, busyAction, scopeAction]
  );

  const updateDraft = (channel: ChannelName, next: Partial<ChannelDraft>) => {
    setDrafts((prev) => ({
      ...prev,
      [channel]: {
        ...prev[channel],
        ...next,
      },
    }));
  };

  const buildChannelInput = useCallback(
    (channel: ChannelName): ChannelConnectionInput => {
      const draft = drafts[channel];
      return {
        token: draft.token.trim() || undefined,
        allowed_users: parseUsersCsv(draft.allowedUsers),
        mention_only: channel === "slack" ? undefined : draft.mentionOnly,
        guild_id: channel === "discord" ? draft.guildId.trim() || null : undefined,
        channel_id: channel === "slack" ? draft.channelId.trim() || null : undefined,
        security_profile: draft.securityProfile.trim() || undefined,
      };
    },
    [drafts]
  );

  const onSave = async (channel: ChannelName) => {
    const input = buildChannelInput(channel);

    setSavingChannel(channel);
    try {
      const next = await setChannelConnection(channel, input);
      applyConnections(next);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save channel settings");
    } finally {
      setSavingChannel(null);
    }
  };

  const formatDiscordVerifyMessage = (result: ChannelVerifyResult): ChannelVerifyFeedback => {
    const checks = result.checks ?? {};
    const tokenOk = checks.token_auth_ok === true;
    const gatewayOk = checks.gateway_ok === true;
    const messageIntentOk = checks.message_content_intent_ok === true;
    const firstHint = result.hints?.[0] ?? "";

    if (result.ok) {
      return {
        tone: "ok",
        text: t("connections.verify.passed", {
          ns: "settings",
          defaultValue:
            "Verification passed: token auth, gateway access, and Message Content Intent are all configured.",
        }),
      };
    }

    const statusLine = t("connections.verify.failedSummary", {
      ns: "settings",
      defaultValue:
        "Verification failed: token={{token}}, gateway={{gateway}}, message intent={{intent}}.",
      token: tokenOk ? "ok" : "fail",
      gateway: gatewayOk ? "ok" : "fail",
      intent: messageIntentOk ? "ok" : "fail",
    });

    return {
      tone: "warn",
      text: firstHint ? `${statusLine} ${firstHint}` : statusLine,
    };
  };

  const onVerify = async (channel: ChannelName) => {
    setVerifyingChannel(channel);
    setVerifyFeedback((prev) => ({
      ...prev,
      [channel]: {
        tone: "warn",
        text: t("connections.verify.running", {
          ns: "settings",
          defaultValue: "Running verification...",
        }),
      },
    }));
    try {
      const result = await verifyChannelConnection(channel, buildChannelInput(channel));
      const feedback =
        channel === "discord"
          ? formatDiscordVerifyMessage(result)
          : {
              tone: result.ok ? "ok" : "warn",
              text: result.ok
                ? t("connections.verify.passedGeneric", {
                    ns: "settings",
                    defaultValue: "Verification passed.",
                  })
                : (result.hints?.[0] ??
                  t("connections.verify.failedGeneric", {
                    ns: "settings",
                    defaultValue: "Verification failed.",
                  })),
            };

      setVerifyFeedback((prev) => ({ ...prev, [channel]: feedback }));
      setError(result.ok ? null : feedback.text);
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to verify channel";
      setVerifyFeedback((prev) => ({
        ...prev,
        [channel]: { tone: "error", text: message },
      }));
      setError(message);
    } finally {
      setVerifyingChannel(null);
    }
  };

  const onDisable = async (channel: ChannelName) => {
    setBusyAction(`disable:${channel}`);
    try {
      const next = await disableChannelConnection(channel);
      applyConnections(next);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to disable channel");
    } finally {
      setBusyAction(null);
    }
  };

  const onForgetToken = async (channel: ChannelName) => {
    setBusyAction(`forget:${channel}`);
    try {
      const next = await deleteChannelConnectionToken(channel);
      applyConnections(next);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete channel token");
    } finally {
      setBusyAction(null);
    }
  };

  const onUpdateToolPreferences = async (
    channel: ChannelName,
    nextPrefs: ChannelToolPreferencesView | { reset: true }
  ) => {
    setScopeAction(`scope:${channel}`);
    try {
      const next =
        "reset" in nextPrefs
          ? await setChannelToolPreferences(channel, { reset: true })
          : await setChannelToolPreferences(channel, {
              enabled_tools: nextPrefs.enabled_tools,
              disabled_tools: nextPrefs.disabled_tools,
              enabled_mcp_servers: nextPrefs.enabled_mcp_servers,
            });
      setToolPreferences((prev) => ({ ...prev, [channel]: next }));
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update tool scope");
    } finally {
      setScopeAction(null);
    }
  };

  if (loading && !connections) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
      </div>
    );
  }

  const current = connections ?? {
    telegram: {
      status: { enabled: false, connected: false, active_sessions: 0 },
      config: { has_token: false, allowed_users: ["*"] },
    },
    discord: {
      status: { enabled: false, connected: false, active_sessions: 0 },
      config: { has_token: false, allowed_users: ["*"], mention_only: true, guild_id: null },
    },
    slack: {
      status: { enabled: false, connected: false, active_sessions: 0 },
      config: { has_token: false, allowed_users: ["*"], channel_id: null },
    },
  };

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold text-text">
            {t("connections.title", { ns: "settings", defaultValue: "Connections" })}
          </h2>
          <p className="text-sm text-text-muted">
            {t("connections.description", {
              ns: "settings",
              defaultValue:
                "Configure Telegram, Discord, and Slack channels. Bot tokens are stored in your encrypted vault.",
            })}
          </p>
          <p className="mt-2 text-xs text-text-subtle">
            {t("connections.runtimeNotice", {
              ns: "settings",
              defaultValue:
                "Desktop channel listeners run only while this app is open and your computer is awake.",
            })}
          </p>
          <p className="text-xs text-text-subtle">
            {t("connections.alwaysOnNotice", {
              ns: "settings",
              defaultValue:
                "For always-on channel automation, deploy Tandem Control Panel/engine on an always-on machine or server.",
            })}
          </p>
        </div>
        <Button size="sm" variant="ghost" onClick={() => void refresh(true)} disabled={isBusy}>
          <RefreshCw className="mr-2 h-4 w-4" />
          {t("actions.refresh", { ns: "common" })}
        </Button>
      </div>

      {error && (
        <div className="rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
          {error}
        </div>
      )}

      {CHANNELS.map((channel) => {
        const channelData = current[channel];
        const draft = drafts[channel];
        const tone = statusTone(channelData.status.connected, channelData.status.enabled);
        const title = t(`connections.channels.${channel}.title`, {
          ns: "settings",
          defaultValue: channel[0].toUpperCase() + channel.slice(1),
        });
        const connectedText = channelData.status.connected
          ? t("connections.status.connected", { ns: "settings", defaultValue: "Connected" })
          : channelData.status.enabled
            ? t("connections.status.configured", { ns: "settings", defaultValue: "Configured" })
            : t("connections.status.notConfigured", {
                ns: "settings",
                defaultValue: "Not configured",
              });

        return (
          <Card key={channel} className={tone.cardBorder}>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div>
                  <CardTitle className="flex items-center gap-2">
                    <Link2 className="h-4 w-4 text-primary" />
                    {title}
                  </CardTitle>
                  <CardDescription className="mt-1 flex items-center gap-2">
                    <span className={`inline-block h-2.5 w-2.5 rounded-full ${tone.dot}`} />
                    <span className={tone.text}>{connectedText}</span>
                    <span className="text-text-subtle">|</span>
                    <span className="text-text-subtle">
                      {t("connections.activeSessions", {
                        ns: "settings",
                        defaultValue: "Active sessions: {{count}}",
                        count: channelData.status.active_sessions,
                      })}
                    </span>
                  </CardDescription>
                </div>
                <div className="rounded-full border border-border px-2 py-1 text-xs text-text-subtle">
                  {channelData.config.has_token
                    ? t("connections.tokenStored", {
                        ns: "settings",
                        defaultValue: "Token stored",
                      })
                    : t("connections.tokenMissing", {
                        ns: "settings",
                        defaultValue: "Token required",
                      })}
                </div>
              </div>
            </CardHeader>
            <CardContent className="space-y-3">
              {(() => {
                const prefs = toolPreferences[channel] ?? defaultToolPreferences();
                const explicitScope = prefs.enabled_tools.length > 0;
                const securityProfile = drafts[channel]?.securityProfile ?? "operator";
                const publicDemo = securityProfile === "public_demo";

                return (
                  <div className="rounded-lg border border-border bg-surface-elevated/30 p-3">
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <p className="text-sm font-medium text-text">Channel tool scope</p>
                        <p className="text-xs text-text-subtle">
                          Control which built-in tools and MCP servers are visible to
                          channel-created sessions.
                        </p>
                        {explicitScope ? (
                          <p className="mt-1 text-xs text-warning">
                            Explicit built-in allowlist is active for this channel.
                          </p>
                        ) : null}
                        {publicDemo ? (
                          <p className="mt-1 text-xs text-text-subtle">
                            Public demo profile can only expose web and quarantined public-memory
                            tools here. File, shell, MCP, and operator-facing tools stay disabled
                            even if saved in channel preferences.
                          </p>
                        ) : null}
                      </div>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void onUpdateToolPreferences(channel, { reset: true })}
                        disabled={isBusy}
                      >
                        Reset scope
                      </Button>
                    </div>

                    <div className="mt-3 space-y-3">
                      {BUILTIN_TOOL_GROUPS.map((group) => (
                        <div key={group.label} className="space-y-2">
                          <p className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                            {group.label}
                          </p>
                          <div className="grid gap-2 md:grid-cols-2">
                            {group.tools.map((tool) => {
                              const allowed = toolAllowedForSecurityProfile(securityProfile, tool);
                              const enabled = toolEnabledForSecurityProfile(
                                prefs,
                                tool,
                                securityProfile
                              );
                              return (
                                <div
                                  key={tool}
                                  className="flex items-center justify-between rounded-lg border border-border bg-background/60 px-3 py-2"
                                >
                                  <div className="flex flex-col">
                                    <span className="font-mono text-xs text-text">{tool}</span>
                                    {!allowed ? (
                                      <span className="text-[11px] text-text-subtle">
                                        Disabled by security profile
                                      </span>
                                    ) : null}
                                  </div>
                                  <Switch
                                    checked={enabled}
                                    onChange={(event) =>
                                      void onUpdateToolPreferences(
                                        channel,
                                        nextToolPreferences(prefs, tool, event.target.checked)
                                      )
                                    }
                                    disabled={isBusy || !allowed}
                                  />
                                </div>
                              );
                            })}
                          </div>
                        </div>
                      ))}

                      <div className="space-y-2">
                        <p className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                          MCP servers
                        </p>
                        {mcpServers.length ? (
                          <div className="grid gap-2 md:grid-cols-2">
                            {mcpServers.map((server) => {
                              const enabled =
                                !publicDemo && prefs.enabled_mcp_servers.includes(server);
                              return (
                                <div
                                  key={server}
                                  className="flex items-center justify-between rounded-lg border border-border bg-background/60 px-3 py-2"
                                >
                                  <div className="flex flex-col">
                                    <span className="font-mono text-xs text-text">{server}</span>
                                    {publicDemo ? (
                                      <span className="text-[11px] text-text-subtle">
                                        Disabled by security profile
                                      </span>
                                    ) : null}
                                  </div>
                                  <Switch
                                    checked={enabled}
                                    onChange={(event) =>
                                      void onUpdateToolPreferences(
                                        channel,
                                        nextMcpServerPreferences(
                                          prefs,
                                          server,
                                          event.target.checked
                                        )
                                      )
                                    }
                                    disabled={isBusy || publicDemo}
                                  />
                                </div>
                              );
                            })}
                          </div>
                        ) : (
                          <p className="text-xs text-text-subtle">
                            {publicDemo
                              ? "MCP servers stay disabled in public demo mode."
                              : "No MCP servers are registered yet."}
                          </p>
                        )}
                      </div>
                    </div>
                  </div>
                );
              })()}

              <div className="space-y-2">
                <label className="text-xs font-medium text-text-muted">
                  {t("connections.botToken", { ns: "settings", defaultValue: "Bot token" })}
                </label>
                <Input
                  type="password"
                  value={draft.token}
                  onChange={(event) => updateDraft(channel, { token: event.target.value })}
                  placeholder={t("connections.botTokenPlaceholder", {
                    ns: "settings",
                    defaultValue:
                      channelData.config.token_masked || "Leave blank to keep saved token",
                  })}
                  autoComplete="off"
                />
                {channelData.config.has_token && !draft.token ? (
                  <p className="text-xs text-text-subtle">
                    {t("connections.tokenStoredHint", {
                      ns: "settings",
                      defaultValue:
                        "A token is already stored. Enter a new one only if you want to replace it.",
                    })}
                  </p>
                ) : null}
              </div>

              <div className="space-y-2">
                <label className="text-xs font-medium text-text-muted">
                  {t("connections.allowedUsers", {
                    ns: "settings",
                    defaultValue: "Allowed users (comma-separated or *)",
                  })}
                </label>
                <Input
                  value={draft.allowedUsers}
                  onChange={(event) => updateDraft(channel, { allowedUsers: event.target.value })}
                />
              </div>

              <div className="space-y-2">
                <label className="text-xs font-medium text-text-muted">Security profile</label>
                <select
                  className="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm"
                  value={draft.securityProfile}
                  onChange={(event) =>
                    updateDraft(channel, { securityProfile: event.target.value })
                  }
                >
                  <option value="operator">Operator</option>
                  <option value="trusted_team">Trusted team</option>
                  <option value="public_demo">Public demo</option>
                </select>
                {draft.securityProfile === "public_demo" ? (
                  <p className="text-xs text-warning">
                    Public demo mode disables operator commands, workspace access, MCP, file tools,
                    and shell access. Memory stays confined to this channel&apos;s quarantined
                    public namespace, and `/help` shows what remains restricted.
                  </p>
                ) : null}
              </div>

              {channel === "discord" && (
                <div className="space-y-2">
                  <label className="text-xs font-medium text-text-muted">
                    {t("connections.guildId", {
                      ns: "settings",
                      defaultValue: "Guild ID (optional)",
                    })}
                  </label>
                  <Input
                    value={draft.guildId}
                    onChange={(event) => updateDraft(channel, { guildId: event.target.value })}
                  />
                </div>
              )}

              {channel === "slack" && (
                <div className="space-y-2">
                  <label className="text-xs font-medium text-text-muted">
                    {t("connections.channelId", { ns: "settings", defaultValue: "Channel ID" })}
                  </label>
                  <Input
                    value={draft.channelId}
                    onChange={(event) => updateDraft(channel, { channelId: event.target.value })}
                  />
                </div>
              )}

              {channel !== "slack" && (
                <div className="flex items-center justify-between rounded-lg border border-border bg-surface-elevated/40 px-3 py-2">
                  <div>
                    <p className="text-sm font-medium text-text">
                      {t("connections.mentionOnly", {
                        ns: "settings",
                        defaultValue: "Mention only",
                      })}
                    </p>
                    <p className="text-xs text-text-subtle">
                      {t("connections.mentionOnlyDescription", {
                        ns: "settings",
                        defaultValue: "Reply only when the bot is mentioned.",
                      })}
                    </p>
                  </div>
                  <Switch
                    checked={draft.mentionOnly}
                    onChange={(event) =>
                      updateDraft(channel, { mentionOnly: event.target.checked })
                    }
                  />
                </div>
              )}

              {channelData.status.last_error && (
                <div className="rounded-lg border border-error/20 bg-error/10 p-3 text-xs text-error">
                  <AlertCircle className="mr-1 inline h-3.5 w-3.5" />
                  {channelData.status.last_error}
                </div>
              )}

              {verifyFeedback[channel] && (
                <div
                  className={`rounded-lg p-3 text-xs ${
                    verifyFeedback[channel].tone === "ok"
                      ? "border border-success/20 bg-success/10 text-success"
                      : verifyFeedback[channel].tone === "error"
                        ? "border border-error/20 bg-error/10 text-error"
                        : "border border-warning/20 bg-warning/10 text-warning"
                  }`}
                >
                  {verifyFeedback[channel].text}
                </div>
              )}

              <div className="flex flex-wrap items-center gap-2 pt-1">
                <Button
                  size="sm"
                  onClick={() => void onSave(channel)}
                  loading={savingChannel === channel}
                  disabled={busyAction !== null}
                >
                  <CheckCircle2 className="mr-2 h-4 w-4" />
                  {channelData.status.enabled
                    ? t("connections.actions.save", { ns: "settings", defaultValue: "Save" })
                    : t("connections.actions.enable", {
                        ns: "settings",
                        defaultValue: "Enable",
                      })}
                </Button>
                {channel === "discord" && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => void onVerify(channel)}
                    loading={verifyingChannel === channel}
                    disabled={isBusy && verifyingChannel !== channel}
                  >
                    {t("connections.actions.verifyDiscord", {
                      ns: "settings",
                      defaultValue: "Verify Discord",
                    })}
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void onDisable(channel)}
                  disabled={isBusy || !channelData.status.enabled}
                >
                  {t("actions.disable", { ns: "common" })}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void onForgetToken(channel)}
                  disabled={isBusy || !channelData.config.has_token}
                >
                  <Trash2 className="mr-2 h-4 w-4" />
                  {t("connections.actions.forgetToken", {
                    ns: "settings",
                    defaultValue: "Forget token",
                  })}
                </Button>
              </div>
            </CardContent>
          </Card>
        );
      })}
    </div>
  );
}
