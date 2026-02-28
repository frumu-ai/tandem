export async function renderChannels(ctx) {
  const { state, byId, toast } = ctx;
  const status = await state.client.channels.status().catch(() => ({}));
  const channels = ["telegram", "discord", "slack"];

  byId("view").innerHTML = '<div class="tcp-card"><div class="mb-3 flex items-center justify-between"><h3 class="tcp-title">Channels</h3><i data-lucide="messages-square"></i></div><div id="channels-list" class="tcp-list"></div></div>';

  const list = byId("channels-list");
  list.innerHTML = channels
    .map((c) => {
      const s = status[c] || {};
      return `
        <div class="tcp-list-item">
          <div class="mb-3 flex items-center justify-between">
            <strong class="capitalize">${c}</strong>
            <span class="${s.connected ? "tcp-badge-ok" : "tcp-badge-warn"}">${s.connected ? "connected" : "not connected"}</span>
          </div>
          <div class="grid gap-3 lg:grid-cols-3">
            <input id="${c}-token" class="tcp-input" placeholder="bot token" />
            <input id="${c}-users" class="tcp-input" placeholder="allowed users (comma)" />
            <div class="flex gap-2">
              <button class="tcp-btn-primary" data-save="${c}"><i data-lucide="save"></i> Save</button>
              <button class="tcp-btn-danger" data-del="${c}"><i data-lucide="trash-2"></i></button>
            </div>
          </div>
        </div>
      `;
    })
    .join("");

  list.querySelectorAll("[data-save]").forEach((btn) =>
    btn.addEventListener("click", async () => {
      const ch = btn.dataset.save;
      const token = byId(`${ch}-token`).value.trim();
      const users = byId(`${ch}-users`).value.trim();
      try {
        await state.client.channels.put(ch, {
          bot_token: token,
          allowed_users: users ? users.split(",").map((v) => v.trim()).filter(Boolean) : ["*"],
        });
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
