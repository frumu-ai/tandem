export async function renderAgents(ctx) {
  const { state, byId, toast, escapeHtml } = ctx;
  const [routinesRaw, automationsRaw] = await Promise.all([
    state.client.routines.list().catch(() => ({ routines: [] })),
    state.client.automations.list().catch(() => ({ automations: [] })),
  ]);
  const routines = routinesRaw.routines || [];
  const automations = automationsRaw.automations || [];

  byId("view").innerHTML = `
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Create Routine</h3>
      <div class="grid gap-3 md:grid-cols-3">
        <input id="routine-name" class="tcp-input" placeholder="Routine name" />
        <input id="routine-cron" class="tcp-input" placeholder="Cron e.g. 0 * * * *" />
        <button id="create-routine" class="tcp-btn-primary"><i data-lucide="plus"></i> Create</button>
      </div>
      <textarea id="routine-prompt" class="tcp-input mt-3" rows="3" placeholder="Entrypoint prompt"></textarea>
    </div>
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Routines (${routines.length})</h3>
      <div id="routine-list" class="tcp-list"></div>
    </div>
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Automations (${automations.length})</h3>
      <div class="tcp-list">${automations.map((r) => `<div class="tcp-list-item flex items-center justify-between gap-2"><span>${escapeHtml(r.name || r.id)}</span><span class="tcp-subtle">${escapeHtml(String(r.status || ""))}</span></div>`).join("") || '<p class="tcp-subtle">No automations.</p>'}</div>
    </div>
  `;

  const routineList = byId("routine-list");
  routineList.innerHTML =
    routines
      .map(
        (r) => `
      <div class="tcp-list-item flex items-center justify-between gap-3">
        <div>
          <div class="font-medium">${escapeHtml(r.name || r.id)}</div>
          <div class="tcp-subtle font-mono">${escapeHtml(typeof r.schedule === "string" ? r.schedule : JSON.stringify(r.schedule || {}))}</div>
        </div>
        <div class="flex gap-2">
          <button data-run="${r.id}" class="tcp-btn"><i data-lucide="play"></i> Run</button>
          <button data-del="${r.id}" class="tcp-btn-danger"><i data-lucide="trash-2"></i></button>
        </div>
      </div>`
      )
      .join("") || '<p class="tcp-subtle">No routines.</p>';

  routineList.querySelectorAll("[data-run]").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        await state.client.routines.runNow(b.dataset.run);
        toast("ok", "Routine triggered.");
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  routineList.querySelectorAll("[data-del]").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        await state.client.routines.delete(b.dataset.del);
        toast("ok", "Routine deleted.");
        renderAgents(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  byId("create-routine").addEventListener("click", async () => {
    try {
      const name = byId("routine-name").value.trim();
      const cron = byId("routine-cron").value.trim();
      const prompt = byId("routine-prompt").value.trim();
      if (!name || !prompt) throw new Error("Name and prompt are required.");
      await state.client.routines.create({
        name,
        entrypoint: prompt,
        schedule: cron ? { type: "cron", cron } : { type: "manual" },
      });
      toast("ok", "Routine created.");
      renderAgents(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });
}
