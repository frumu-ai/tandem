import React, { useState, useEffect } from "react";
import { api } from "../api";
import { Cable, Save, RefreshCw } from "lucide-react";

export const ConnectorsDashboard: React.FC = () => {
  const [config, setConfig] = useState<any>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const [telegramToken, setTelegramToken] = useState("");
  const [discordToken, setDiscordToken] = useState("");
  const [slackBotToken, setSlackBotToken] = useState("");
  const [slackAppToken, setSlackAppToken] = useState("");

  const fetchConfig = async () => {
    try {
      const res = await fetch("/engine/channels/config", {
        headers: { Authorization: `Bearer ${api["token"]}` }, // using the raw token directly via accessor in real app
      });
      if (res.ok) {
        const data = await res.json();
        setConfig(data);

        // Set local state
        if (data.telegram?.bot_token) setTelegramToken(data.telegram.bot_token);
        if (data.discord?.bot_token) setDiscordToken(data.discord.bot_token);
        if (data.slack?.bot_token) setSlackBotToken(data.slack.bot_token);
        if (data.slack?.app_token) setSlackAppToken(data.slack.app_token);
      }
    } catch (error) {
      console.error("Failed to load channel config", error);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchConfig();
  }, []);

  const handleSave = async (channel: "telegram" | "discord" | "slack", payload: any) => {
    setSaving(true);
    try {
      await fetch(`/engine/channels/${channel}`, {
        method: "PUT",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${api["token"]}`,
        },
        body: JSON.stringify(payload),
      });
      await fetchConfig(); // Refresh status
    } catch (error) {
      console.error(`Failed to save ${channel} config`, error);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex flex-col h-full bg-gray-950 p-6 overflow-y-auto">
      <div className="mb-8">
        <h2 className="text-2xl font-bold text-white flex items-center gap-2">
          <Cable className="text-orange-500" />
          Connectors & Channels
        </h2>
        <p className="text-gray-400 mt-1">
          Manage bot tokens to connect the engine directly to your team's chat platforms.
        </p>
      </div>

      {loading ? (
        <div className="text-gray-500 animate-pulse">Loading engine configuration...</div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {/* TELEGRAM */}
          <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 shadow-sm">
            <div className="flex justify-between items-center mb-4">
              <h3 className="text-lg font-bold text-white">Telegram</h3>
              <span
                className={`px-2 py-1 text-xs font-medium rounded-full ${config?.telegram?.enabled ? "bg-emerald-900/50 text-emerald-400 border border-emerald-800" : "bg-gray-800 text-gray-400"}`}
              >
                {config?.telegram?.enabled ? "Active" : "Disabled"}
              </span>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">Bot Token</label>
                <input
                  type="password"
                  value={telegramToken}
                  onChange={(e) => setTelegramToken(e.target.value)}
                  className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white focus:ring-1 focus:ring-orange-500"
                  placeholder="123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"
                />
              </div>
              <button
                onClick={() => handleSave("telegram", { bot_token: telegramToken })}
                disabled={saving}
                className="w-full bg-gray-800 hover:bg-gray-700 text-white rounded-md py-2 text-sm font-medium transition flex items-center justify-center gap-2"
              >
                {saving ? <RefreshCw className="animate-spin" size={16} /> : <Save size={16} />}
                Save & Restart Listener
              </button>
            </div>
          </div>

          {/* DISCORD */}
          <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 shadow-sm">
            <div className="flex justify-between items-center mb-4">
              <h3 className="text-lg font-bold text-white">Discord</h3>
              <span
                className={`px-2 py-1 text-xs font-medium rounded-full ${config?.discord?.enabled ? "bg-emerald-900/50 text-emerald-400 border border-emerald-800" : "bg-gray-800 text-gray-400"}`}
              >
                {config?.discord?.enabled ? "Active" : "Disabled"}
              </span>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">Bot Token</label>
                <input
                  type="password"
                  value={discordToken}
                  onChange={(e) => setDiscordToken(e.target.value)}
                  className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white focus:ring-1 focus:ring-orange-500"
                  placeholder="MTEz..."
                />
              </div>
              <button
                onClick={() => handleSave("discord", { bot_token: discordToken })}
                disabled={saving}
                className="w-full bg-gray-800 hover:bg-gray-700 text-white rounded-md py-2 text-sm font-medium transition flex items-center justify-center gap-2"
              >
                {saving ? <RefreshCw className="animate-spin" size={16} /> : <Save size={16} />}
                Save & Restart Listener
              </button>
            </div>
          </div>

          {/* SLACK */}
          <div className="bg-gray-900 border border-gray-800 rounded-xl p-5 shadow-sm">
            <div className="flex justify-between items-center mb-4">
              <h3 className="text-lg font-bold text-white">Slack</h3>
              <span
                className={`px-2 py-1 text-xs font-medium rounded-full ${config?.slack?.enabled ? "bg-emerald-900/50 text-emerald-400 border border-emerald-800" : "bg-gray-800 text-gray-400"}`}
              >
                {config?.slack?.enabled ? "Active" : "Disabled"}
              </span>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">
                  Bot Token (xoxb-)
                </label>
                <input
                  type="password"
                  value={slackBotToken}
                  onChange={(e) => setSlackBotToken(e.target.value)}
                  className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white focus:ring-1 focus:ring-orange-500"
                  placeholder="xoxb-..."
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1">
                  App Token (xapp-)
                </label>
                <input
                  type="password"
                  value={slackAppToken}
                  onChange={(e) => setSlackAppToken(e.target.value)}
                  className="w-full bg-gray-950 border border-gray-700 rounded-md px-3 py-2 text-sm text-white focus:ring-1 focus:ring-orange-500"
                  placeholder="xapp-..."
                />
              </div>
              <button
                onClick={() =>
                  handleSave("slack", { bot_token: slackBotToken, app_token: slackAppToken })
                }
                disabled={saving}
                className="w-full bg-gray-800 hover:bg-gray-700 text-white rounded-md py-2 text-sm font-medium transition flex items-center justify-center gap-2"
              >
                {saving ? <RefreshCw className="animate-spin" size={16} /> : <Save size={16} />}
                Save & Restart Listener
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
