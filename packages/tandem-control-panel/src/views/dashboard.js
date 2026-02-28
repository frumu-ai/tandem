export async function renderDashboard(ctx) {
  const { api, state, byId, escapeHtml, setRoute } = ctx;
  const health = await api("/api/system/health").catch(() => ({}));
  const provider = await state.client.providers.config().catch(() => ({ default: null, providers: {} }));
  const channels = await state.client.channels.status().catch(() => ({}));
  const routines = await state.client.routines.list().catch(() => ({ routines: [] }));
  const automations = await state.client.automations.list().catch(() => ({ automations: [] }));

  byId("view").innerHTML = `
    <div class="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
      <div class="tcp-card">
        <div class="mb-2 flex items-center justify-between"><span class="tcp-subtle">Engine</span><i data-lucide="cpu"></i></div>
        <div class="text-2xl font-semibold">${escapeHtml(health.engine?.version || "unknown")}</div>
        <p class="mt-1 text-sm ${health.engine?.ready || health.engine?.healthy ? "text-lime-300" : "text-rose-300"}">${
          health.engine?.ready || health.engine?.healthy ? "Healthy" : "Unhealthy"
        }</p>
      </div>
      <div class="tcp-card">
        <div class="mb-2 flex items-center justify-between"><span class="tcp-subtle">Provider</span><i data-lucide="bot"></i></div>
        <div class="text-2xl font-semibold">${escapeHtml(provider.default || "none")}</div>
        <p class="mt-1 text-sm text-slate-400">Default model configured</p>
      </div>
      <div class="tcp-card">
        <div class="mb-2 flex items-center justify-between"><span class="tcp-subtle">Channels</span><i data-lucide="messages-square"></i></div>
        <div class="text-2xl font-semibold">${Object.values(channels || {}).filter((c) => c?.connected).length}</div>
        <p class="mt-1 text-sm text-slate-400">Connected integrations</p>
      </div>
      <div class="tcp-card">
        <div class="mb-2 flex items-center justify-between"><span class="tcp-subtle">Scheduled</span><i data-lucide="clock-3"></i></div>
        <div class="text-2xl font-semibold">${(routines.routines || []).length + (automations.automations || []).length}</div>
        <p class="mt-1 text-sm text-slate-400">Routines + automations</p>
      </div>
    </div>
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Quick Actions</h3>
      <div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <button class="tcp-btn w-full justify-start" data-goto="chat"><i data-lucide="message-square"></i> Open Chat</button>
        <button class="tcp-btn w-full justify-start" data-goto="agents"><i data-lucide="clipboard-list"></i> Manage Routines</button>
        <button class="tcp-btn w-full justify-start" data-goto="swarm"><i data-lucide="workflow"></i> Launch Swarm</button>
        <button class="tcp-btn w-full justify-start" data-goto="mcp"><i data-lucide="plug-zap"></i> Connect MCP</button>
      </div>
    </div>
  `;

  byId("view").querySelectorAll("[data-goto]").forEach((btn) => {
    btn.addEventListener("click", () => setRoute(btn.dataset.goto));
  });
}
