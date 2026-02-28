export async function renderMcp(ctx) {
  const { state, byId, toast, escapeHtml } = ctx;
  const [servers, tools] = await Promise.all([
    state.client.mcp.list().catch(() => ({})),
    state.client.mcp.listTools().catch(() => []),
  ]);

  byId("view").innerHTML = `
    <div class="grid gap-4 xl:grid-cols-[420px_1fr]">
      <div class="tcp-card">
        <h3 class="tcp-title mb-3">Add MCP Server</h3>
        <div class="grid gap-3">
          <input id="mcp-name" class="tcp-input" placeholder="name" value="arcade" />
          <input id="mcp-transport" class="tcp-input" placeholder="https://.../mcp or stdio:..." />
          <button id="mcp-add" class="tcp-btn-primary"><i data-lucide="link"></i> Add + Connect</button>
        </div>
      </div>
      <div class="grid gap-4">
        <div class="tcp-card">
          <h3 class="tcp-title mb-3">Servers</h3>
          <div id="mcp-servers" class="tcp-list"></div>
        </div>
        <div class="tcp-card">
          <h3 class="tcp-title mb-3">MCP Tools (${tools.length})</h3>
          <pre class="tcp-code max-h-[280px] overflow-auto">${escapeHtml(tools.slice(0, 200).map((t) => t.id || JSON.stringify(t)).join("\n"))}</pre>
        </div>
      </div>
    </div>
  `;

  byId("mcp-add").addEventListener("click", async () => {
    const name = byId("mcp-name").value.trim();
    const transport = byId("mcp-transport").value.trim();
    if (!name || !transport) return toast("err", "name and transport are required");
    try {
      await state.client.mcp.add({ name, transport, enabled: true });
      await state.client.mcp.connect(name);
      toast("ok", "MCP connected.");
      renderMcp(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });

  const list = byId("mcp-servers");
  const rows = Object.entries(servers || {});
  list.innerHTML =
    rows
      .map(
        ([name, cfg]) => `
      <div class="tcp-list-item flex items-center justify-between gap-3">
        <div><strong>${escapeHtml(name)}</strong><div class="tcp-subtle">${escapeHtml(cfg.transport || "")}</div></div>
        <div class="flex gap-2">
          <button data-c="${name}" class="tcp-btn">Connect</button>
          <button data-r="${name}" class="tcp-btn">Refresh</button>
          <button data-d="${name}" class="tcp-btn-danger">Delete</button>
        </div>
      </div>
    `
      )
      .join("") || '<p class="tcp-subtle">No MCP servers configured.</p>';

  list.querySelectorAll("[data-c]").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        await state.client.mcp.connect(b.dataset.c);
        toast("ok", "Connected.");
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  list.querySelectorAll("[data-r]").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        await state.client.mcp.refresh(b.dataset.r);
        toast("ok", "Refreshed.");
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  list.querySelectorAll("[data-d]").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        await state.client.mcp.delete(b.dataset.d);
        toast("ok", "Deleted.");
        renderMcp(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );
}
