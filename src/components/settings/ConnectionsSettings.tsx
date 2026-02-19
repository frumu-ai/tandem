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
  onSidecarEventV2,
  setChannelConnection,
  type ChannelConnectionInput,
  type ChannelConnectionsView,
  type ChannelName,
  type StreamEventEnvelopeV2,
} from "@/lib/tauri";

type ChannelDraft = {
  token: string;
  allowedUsers: string;
  mentionOnly: boolean;
  guildId: string;
  channelId: string;
};

type ChannelDrafts = Record<ChannelName, ChannelDraft>;

const CHANNELS: ChannelName[] = ["telegram", "discord", "slack"];

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
    },
    discord: {
      token: "",
      allowedUsers: toCsv(connections.discord.config.allowed_users),
      mentionOnly: connections.discord.config.mention_only ?? true,
      guildId: connections.discord.config.guild_id ?? "",
      channelId: "",
    },
    slack: {
      token: "",
      allowedUsers: toCsv(connections.slack.config.allowed_users),
      mentionOnly: false,
      guildId: "",
      channelId: connections.slack.config.channel_id ?? "",
    },
  };
}

function defaultDrafts(): ChannelDrafts {
  return {
    telegram: { token: "", allowedUsers: "*", mentionOnly: false, guildId: "", channelId: "" },
    discord: { token: "", allowedUsers: "*", mentionOnly: true, guildId: "", channelId: "" },
    slack: { token: "", allowedUsers: "*", mentionOnly: false, guildId: "", channelId: "" },
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
  const [busyAction, setBusyAction] = useState<string | null>(null);

  const applyConnections = useCallback((next: ChannelConnectionsView) => {
    setConnections(next);
    setDrafts(draftFromConnections(next));
  }, []);

  const refresh = useCallback(
    async (showSpinner = false) => {
      if (showSpinner) setLoading(true);
      try {
        const next = await getChannelConnections();
        applyConnections(next);
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
    () => savingChannel !== null || busyAction !== null,
    [savingChannel, busyAction]
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

  const onSave = async (channel: ChannelName) => {
    const draft = drafts[channel];
    const input: ChannelConnectionInput = {
      token: draft.token.trim() || undefined,
      allowed_users: parseUsersCsv(draft.allowedUsers),
      mention_only: channel === "slack" ? undefined : draft.mentionOnly,
      guild_id: channel === "discord" ? draft.guildId.trim() || null : undefined,
      channel_id: channel === "slack" ? draft.channelId.trim() || null : undefined,
    };

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
                    defaultValue: "Leave blank to keep saved token",
                  })}
                  autoComplete="off"
                />
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
