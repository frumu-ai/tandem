export async function renderChannels(ctx) {
  const { state, byId, toast, escapeHtml } = ctx;
  const [status, config] = await Promise.all([
    state.client.channels.status().catch(() => ({})),
    state.client.channels.config().catch(() => ({})),
  ]);
  const channels = ["telegram", "discord", "slack"];
  const readField = (obj, snake, camel, fallback = undefined) => {
    if (!obj || typeof obj !== "object") return fallback;
    if (obj[snake] !== undefined) return obj[snake];
    if (obj[camel] !== undefined) return obj[camel];
    return fallback;
  };
  const usersCsv = (raw) => (Array.isArray(raw) && raw.length ? raw.join(", ") : "*");

  byId("view").innerHTML = '<div class="tcp-card"><div class="mb-3 flex items-center justify-between"><h3 class="tcp-title">Channels</h3><i data-lucide="messages-square"></i></div><div id="channels-list" class="tcp-list"></div></div>';

  const list = byId("channels-list");
  list.innerHTML = channels
    .map((c) => {
      const s = status[c] || {};
      const lastError = readField(s, "last_error", "lastError", "");
      return `
        <div class="tcp-list-item">
          <div class="mb-3 flex items-center justify-between">
            <strong class="capitalize">${c}</strong>
            <span class="${s.connected ? "tcp-badge-ok" : "tcp-badge-warn"}">${s.connected ? "connected" : "not connected"}</span>
          </div>
          <div class="grid gap-3 lg:grid-cols-4">
            <input id="${c}-token" class="tcp-input" placeholder="bot token" />
            <input id="${c}-users" class="tcp-input" placeholder="allowed users (comma, * for all)" />
            ${
              c === "discord"
                ? `<input id="${c}-guild" class="tcp-input" placeholder="guild id (optional)" />`
                : c === "slack"
                  ? `<input id="${c}-channel" class="tcp-input" placeholder="channel id (required for slack)" />`
                  : `<div></div>`
            }
            <div class="flex gap-2">
              <button class="tcp-btn-primary" data-save="${c}"><i data-lucide="save"></i> Save</button>
              <button class="tcp-btn-danger" data-del="${c}"><i data-lucide="trash-2"></i></button>
            </div>
          </div>
          <div class="mt-2 flex flex-wrap items-center gap-3 text-xs text-slate-400">
            ${c === "telegram" || c === "discord" ? `<label class="inline-flex items-center gap-2"><input id="${c}-mention" type="checkbox" /> mention only</label>` : ""}
            ${c === "discord" ? `<span>Tip: use <code>@bot /help</code> style commands (Discord app slash commands are not registered).</span>` : ""}
          </div>
          ${lastError ? `<div class="mt-2 text-xs text-rose-300">last error: ${escapeHtml(String(lastError))}</div>` : ""}
        </div>
      `;
    })
    .join("");

  channels.forEach((c) => {
    const cfg = config[c] || {};
    const users = readField(cfg, "allowed_users", "allowedUsers", ["*"]);
    const mentionOnly = !!readField(
      cfg,
      "mention_only",
      "mentionOnly",
      c === "discord"
    );
    const guildId = readField(cfg, "guild_id", "guildId", "");
    const channelId = readField(cfg, "channel_id", "channelId", "");
    const hasToken = !!readField(cfg, "has_token", "hasToken", false);

    const tokenEl = byId(`${c}-token`);
    if (tokenEl && hasToken) tokenEl.placeholder = "token configured (leave blank to keep)";
    const usersEl = byId(`${c}-users`);
    if (usersEl) usersEl.value = usersCsv(users);
    const mentionEl = byId(`${c}-mention`);
    if (mentionEl) mentionEl.checked = mentionOnly;
    const guildEl = byId(`${c}-guild`);
    if (guildEl) guildEl.value = guildId || "";
    const channelEl = byId(`${c}-channel`);
    if (channelEl) channelEl.value = channelId || "";
  });

  list.querySelectorAll("[data-save]").forEach((btn) =>
    btn.addEventListener("click", async () => {
      const ch = btn.dataset.save;
      const token = byId(`${ch}-token`).value.trim();
      const users = byId(`${ch}-users`).value.trim();
      const payload = {
        bot_token: token,
        allowed_users: users ? users.split(",").map((v) => v.trim()).filter(Boolean) : ["*"],
      };
      if (ch === "telegram" || ch === "discord") {
        payload.mention_only = !!byId(`${ch}-mention`)?.checked;
      }
      if (ch === "discord") {
        payload.guild_id = byId(`${ch}-guild`)?.value?.trim() || null;
      }
      if (ch === "slack") {
        payload.channel_id = byId(`${ch}-channel`)?.value?.trim() || null;
      }
      try {
        await state.client.channels.put(ch, payload);
        toast("ok", `${ch} saved.`);
        renderChannels(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  list.querySelectorAll("[data-del]").forEach((btn) =>
    btn.addEventListener("click", async () => {
      const ch = btn.dataset.del;
      try {
        await state.client.channels.delete(ch);
        toast("ok", `${ch} deleted.`);
        renderChannels(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );
}
