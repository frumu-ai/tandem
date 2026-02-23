import React, { useEffect, useState } from "react";
import { api, type ChannelsConfigResponse, type ChannelsStatusResponse } from "../api";
import { Cable, Save, RefreshCw, Trash2 } from "lucide-react";

export const ConnectorsDashboard: React.FC = () => {
  const [config, setConfig] = useState<ChannelsConfigResponse | null>(null);
  const [status, setStatus] = useState<ChannelsStatusResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [unsupported, setUnsupported] = useState(false);

  const [telegramToken, setTelegramToken] = useState("");
  const [telegramUsers, setTelegramUsers] = useState("*");
  const [discordToken, setDiscordToken] = useState("");
  const [discordUsers, setDiscordUsers] = useState("*");
  const [discordGuild, setDiscordGuild] = useState("");
  const [slackToken, setSlackToken] = useState("");
  const [slackChannel, setSlackChannel] = useState("");
  const [slackUsers, setSlackUsers] = useState("*");
  const [telegramHasToken, setTelegramHasToken] = useState(false);
  const [discordHasToken, setDiscordHasToken] = useState(false);
  const [slackHasToken, setSlackHasToken] = useState(false);

  const load = async () => {
    setLoading(true);
    setError("");
    try {
      const [cfg, st] = await Promise.all([api.getChannelsConfig(), api.getChannelsStatus()]);
      setConfig(cfg);
      setStatus(st);
      setUnsupported(false);

      setTelegramUsers((cfg.telegram.allowed_users || ["*"]).join(","));
      setDiscordUsers((cfg.discord.allowed_users || ["*"]).join(","));
      setDiscordGuild(cfg.discord.guild_id || "");
      setSlackUsers((cfg.slack.allowed_users || ["*"]).join(","));
      setSlackChannel(cfg.slack.channel_id || "");
      setTelegramHasToken(!!cfg.telegram.has_token);
      setDiscordHasToken(!!cfg.discord.has_token);
      setSlackHasToken(!!cfg.slack.has_token);

      // Never show persisted secret values in the input field.
      setTelegramToken("");
      setDiscordToken("");
      setSlackToken("");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (message.includes("404")) {
        setUnsupported(true);
      } else {
        setError("Failed to load channel connector config from engine.");
      }
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
  }, []);

  const save = async (channel: "telegram" | "discord" | "slack") => {
    if (unsupported) return;
    setSaving(true);
    setError("");
    try {
      if (channel === "telegram") {
        await api.putChannel("telegram", {
          bot_token: telegramToken,
          allowed_users: telegramUsers
            .split(",")
            .map((v) => v.trim())
            .filter(Boolean),
          mention_only: false,
        });
      }
      if (channel === "discord") {
        await api.putChannel("discord", {
          bot_token: discordToken,
          guild_id: discordGuild || null,
          allowed_users: discordUsers
            .split(",")
            .map((v) => v.trim())
            .filter(Boolean),
          mention_only: true,
        });
      }
      if (channel === "slack") {
        await api.putChannel("slack", {
          bot_token: slackToken,
          channel_id: slackChannel,
          allowed_users: slackUsers
            .split(",")
            .map((v) => v.trim())
            .filter(Boolean),
        });
      }
      await load();
    } catch {
      setError(`Failed to save ${channel} connector config.`);
    } finally {
      setSaving(false);
    }
  };

  const remove = async (channel: "telegram" | "discord" | "slack") => {
    setSaving(true);
    setError("");
    try {
      await api.deleteChannel(channel);
      await load();
    } catch {
      setError(`Failed to remove ${channel} connector config.`);
    } finally {
      setSaving(false);
    }
  };

  const stateBadge = (name: "telegram" | "discord" | "slack") => {
    const s = status?.[name];
    if (!s) return "Unknown";
    if (!s.enabled) return "Disabled";
    if (s.connected) return "Active";
    return "Not Connected";
  };

  return (
    <div className="flex flex-col h-full bg-gray-950 p-6 overflow-y-auto">
      <div className="mb-8">
        <h2 className="text-2xl font-bold text-white flex items-center gap-2">
          <Cable className="text-orange-500" />
          Connectors & Channels
        </h2>
        <p className="text-gray-400 mt-1">
          Manage bot tokens and allowlists for Telegram, Discord, and Slack listeners.
        </p>
      </div>

      {loading ? (
        <div className="text-gray-500 animate-pulse">Loading engine configuration...</div>
      ) : unsupported ? (
        <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 text-gray-300">
          <p className="text-sm">
            Channel connector endpoints are not available in this engine build.
          </p>
          <p className="text-xs text-gray-500 mt-2">
            Upgrade tandem-engine to a build with <code>/channels/*</code> endpoints.
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 shadow-sm space-y-4">
            <div className="flex justify-between items-center">
              <h3 className="text-lg font-bold text-white">Telegram</h3>
              <span className="px-2 py-1 text-xs rounded bg-gray-800 text-gray-300">
                {stateBadge("telegram")}
              </span>
            </div>
            <input
              type="password"
              value={telegramToken}
              onChange={(e) => setTelegramToken(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder={
                telegramHasToken ? "Token configured (leave blank to keep)" : "Bot token"
              }
            />
            <input
              value={telegramUsers}
              onChange={(e) => setTelegramUsers(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder="@alice,@bob,123456789,*"
            />
            <p className="text-xs text-gray-500">
              Allowed users are exact sender IDs: Telegram usernames (`@name`) or numeric account
              IDs. Use `*` to allow everyone.
            </p>
            <div className="flex gap-2">
              <button
                onClick={() => void save("telegram")}
                disabled={saving}
                className="flex-1 bg-gray-800 hover:bg-gray-700 text-white rounded-md py-2 text-sm font-medium transition flex items-center justify-center gap-2"
              >
                {saving ? <RefreshCw className="animate-spin" size={16} /> : <Save size={16} />}
                Save
              </button>
              <button
                onClick={() => void remove("telegram")}
                disabled={saving}
                className="bg-red-900/60 hover:bg-red-900 text-red-100 rounded-md py-2 px-3 text-sm"
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>

          <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 shadow-sm space-y-4">
            <div className="flex justify-between items-center">
              <h3 className="text-lg font-bold text-white">Discord</h3>
              <span className="px-2 py-1 text-xs rounded bg-gray-800 text-gray-300">
                {stateBadge("discord")}
              </span>
            </div>
            <input
              type="password"
              value={discordToken}
              onChange={(e) => setDiscordToken(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder={discordHasToken ? "Token configured (leave blank to keep)" : "Bot token"}
            />
            <input
              value={discordGuild}
              onChange={(e) => setDiscordGuild(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder="Guild ID (optional)"
            />
            <input
              value={discordUsers}
              onChange={(e) => setDiscordUsers(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder="123456789012345678,*"
            />
            <div className="flex gap-2">
              <button
                onClick={() => void save("discord")}
                disabled={saving}
                className="flex-1 bg-gray-800 hover:bg-gray-700 text-white rounded-md py-2 text-sm font-medium transition flex items-center justify-center gap-2"
              >
                {saving ? <RefreshCw className="animate-spin" size={16} /> : <Save size={16} />}
                Save
              </button>
              <button
                onClick={() => void remove("discord")}
                disabled={saving}
                className="bg-red-900/60 hover:bg-red-900 text-red-100 rounded-md py-2 px-3 text-sm"
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>

          <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 shadow-sm space-y-4">
            <div className="flex justify-between items-center">
              <h3 className="text-lg font-bold text-white">Slack</h3>
              <span className="px-2 py-1 text-xs rounded bg-gray-800 text-gray-300">
                {stateBadge("slack")}
              </span>
            </div>
            <input
              type="password"
              value={slackToken}
              onChange={(e) => setSlackToken(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder={slackHasToken ? "Token configured (leave blank to keep)" : "xoxb-..."}
            />
            <input
              value={slackChannel}
              onChange={(e) => setSlackChannel(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder="Channel ID"
            />
            <input
              value={slackUsers}
              onChange={(e) => setSlackUsers(e.target.value)}
              className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white"
              placeholder="U01A,U02B,*"
            />
            <div className="flex gap-2">
              <button
                onClick={() => void save("slack")}
                disabled={saving}
                className="flex-1 bg-gray-800 hover:bg-gray-700 text-white rounded-md py-2 text-sm font-medium transition flex items-center justify-center gap-2"
              >
                {saving ? <RefreshCw className="animate-spin" size={16} /> : <Save size={16} />}
                Save
              </button>
              <button
                onClick={() => void remove("slack")}
                disabled={saving}
                className="bg-red-900/60 hover:bg-red-900 text-red-100 rounded-md py-2 px-3 text-sm"
              >
                <Trash2 size={14} />
              </button>
            </div>
          </div>
        </div>
      )}

      <div className="mt-4 flex gap-2">
        <button
          onClick={() => void load()}
          className="bg-gray-800 hover:bg-gray-700 text-white rounded-md py-2 px-4 text-sm"
        >
          Refresh
        </button>
      </div>

      {!loading && !unsupported && error && (
        <div className="mt-4 text-sm text-red-400">{error}</div>
      )}
      {!loading && !unsupported && (
        <div className="mt-4 grid md:grid-cols-2 gap-4">
          <pre className="text-xs bg-gray-900 border border-gray-800 rounded p-3 overflow-auto">
            {JSON.stringify(config || {}, null, 2)}
          </pre>
          <pre className="text-xs bg-gray-900 border border-gray-800 rounded p-3 overflow-auto">
            {JSON.stringify(status || {}, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
};
