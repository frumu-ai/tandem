export async function renderMemory(ctx) {
  const { state, byId, toast, escapeHtml } = ctx;
  const data = await state.client.memory.list({ limit: 100 }).catch(() => ({ items: [] }));
  const items = data.items || [];

  byId("view").innerHTML = `
    <div class="tcp-card">
      <div class="mb-3 flex items-center justify-between">
        <h3 class="tcp-title">Memory</h3>
        <span class="tcp-badge-info">${items.length} records</span>
      </div>
      <div class="grid gap-3 md:grid-cols-[1fr_auto_auto]">
        <input id="mem-query" class="tcp-input" placeholder="Search query" />
        <button id="mem-search" class="tcp-btn-primary"><i data-lucide="search"></i> Search</button>
        <button id="mem-refresh" class="tcp-btn"><i data-lucide="refresh-cw"></i></button>
      </div>
      <div id="mem-results" class="tcp-list mt-3"></div>
    </div>
  `;

  const renderRows = (rows) => {
    byId("mem-results").innerHTML =
      rows
        .map(
          (m) => `
      <div class="tcp-list-item flex items-center justify-between gap-3">
        <div>
          <strong class="font-mono text-xs text-slate-300">${escapeHtml(m.id || "(no id)")}</strong>
          <div class="tcp-subtle mt-1">${escapeHtml((m.text || m.content || "").slice(0, 140))}</div>
        </div>
        <button data-del="${escapeHtml(m.id || "")}" class="tcp-btn-danger"><i data-lucide="trash-2"></i></button>
      </div>
    `
        )
        .join("") || '<p class="tcp-subtle">No memory records.</p>';

    byId("mem-results").querySelectorAll("[data-del]").forEach((btn) =>
      btn.addEventListener("click", async () => {
        const id = btn.dataset.del;
        if (!id) return;
        try {
          await state.client.memory.delete(id);
          toast("ok", "Memory deleted.");
          renderMemory(ctx);
        } catch (e) {
          toast("err", e instanceof Error ? e.message : String(e));
        }
      })
    );
  };

  renderRows(items);

  byId("mem-refresh").addEventListener("click", () => renderMemory(ctx));
  byId("mem-search").addEventListener("click", async () => {
    const q = byId("mem-query").value.trim();
    if (!q) return renderRows(items);
    try {
      const result = await state.client.memory.search({ query: q, limit: 50 });
      renderRows(result.results || []);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });
}
