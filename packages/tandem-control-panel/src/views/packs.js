export async function renderPacks(ctx) {
  const { byId, state, toast, renderIcons, escapeHtml } = ctx;
  const app = byId("view");
  app.innerHTML = `
    <div class="tcp-card mb-4">
      <h3 class="tcp-title mb-2">Pack Library</h3>
      <p class="tcp-subtle mb-3">Install, inspect, export, and remove Tandem Packs.</p>
      <div class="grid gap-2 md:grid-cols-3">
        <input id="packs-install-url" class="tcp-input" placeholder="Install from URL (https://...zip)" />
        <input id="packs-install-path" class="tcp-input" placeholder="Install from server path (/path/to/pack.zip)" />
        <button id="packs-install-btn" class="tcp-btn-primary"><i data-lucide="download"></i> Install</button>
      </div>
      <div class="mt-3 flex flex-wrap gap-2">
        <button id="packs-refresh-btn" class="tcp-btn"><i data-lucide="refresh-cw"></i> Refresh</button>
        <button id="packs-cap-discovery-btn" class="tcp-btn"><i data-lucide="binary"></i> Capability Discovery</button>
      </div>
      <pre id="packs-meta" class="mt-3 rounded-xl border border-slate-800 bg-slate-950/70 p-3 text-xs text-slate-300 hidden"></pre>
    </div>
    <div id="packs-list" class="grid gap-3"></div>
  `;

  const packsListEl = byId("packs-list");
  const metaEl = byId("packs-meta");
  const setMeta = (value) => {
    if (!metaEl) return;
    if (!value) {
      metaEl.classList.add("hidden");
      metaEl.textContent = "";
      return;
    }
    metaEl.classList.remove("hidden");
    metaEl.textContent = JSON.stringify(value, null, 2);
  };

  const loadPacks = async () => {
    try {
      const payload = await state.client.packs.list();
      const packs = Array.isArray(payload?.packs) ? payload.packs : [];
      if (!packs.length) {
        packsListEl.innerHTML = '<div class="tcp-card"><p class="tcp-subtle">No installed packs.</p></div>';
        renderIcons();
        return;
      }
      packsListEl.innerHTML = packs
        .map((pack) => {
          const id = String(pack.pack_id || pack.name || "");
          const name = String(pack.name || "unknown");
          const version = String(pack.version || "unknown");
          const type = String(pack.pack_type || "unknown");
          return `
          <div class="tcp-card">
            <div class="flex items-start justify-between gap-3">
              <div>
                <h4 class="font-semibold">${escapeHtml(name)} <span class="tcp-subtle">(${escapeHtml(version)})</span></h4>
                <p class="tcp-subtle text-xs">pack_id: <span class="font-mono">${escapeHtml(id)}</span> · type: ${escapeHtml(type)}</p>
              </div>
              <div class="flex flex-wrap gap-2">
                <button class="tcp-btn" data-pack-inspect="${escapeHtml(id)}"><i data-lucide="search"></i> Inspect</button>
                <button class="tcp-btn" data-pack-export="${escapeHtml(id)}"><i data-lucide="archive"></i> Export</button>
                <button class="tcp-btn" data-pack-updates="${escapeHtml(id)}"><i data-lucide="badge-check"></i> Updates</button>
                <button class="tcp-btn" data-pack-update="${escapeHtml(id)}"><i data-lucide="arrow-up-circle"></i> Update</button>
                <button class="tcp-btn" data-pack-remove="${escapeHtml(id)}"><i data-lucide="trash-2"></i> Uninstall</button>
              </div>
            </div>
          </div>`;
        })
        .join("");
      renderIcons();

      packsListEl.querySelectorAll("[data-pack-inspect]").forEach((btn) => {
        btn.addEventListener("click", async () => {
          const id = btn.getAttribute("data-pack-inspect");
          if (!id) return;
          try {
            const inspected = await state.client.packs.inspect(id);
            setMeta(inspected);
          } catch (e) {
            toast("err", `Inspect failed: ${e instanceof Error ? e.message : String(e)}`);
          }
        });
      });

      packsListEl.querySelectorAll("[data-pack-export]").forEach((btn) => {
        btn.addEventListener("click", async () => {
          const id = btn.getAttribute("data-pack-export");
          if (!id) return;
          try {
            const exported = await state.client.packs.export({ pack_id: id });
            toast("ok", `Exported to ${exported?.exported?.path || "unknown path"}`);
          } catch (e) {
            toast("err", `Export failed: ${e instanceof Error ? e.message : String(e)}`);
          }
        });
      });

      packsListEl.querySelectorAll("[data-pack-updates]").forEach((btn) => {
        btn.addEventListener("click", async () => {
          const id = btn.getAttribute("data-pack-updates");
          if (!id) return;
          try {
            const updates = await state.client.packs.updates(id);
            setMeta(updates);
            const count = Array.isArray(updates?.updates) ? updates.updates.length : 0;
            toast("info", `Updates available: ${count}`);
          } catch (e) {
            toast("err", `Updates check failed: ${e instanceof Error ? e.message : String(e)}`);
          }
        });
      });

      packsListEl.querySelectorAll("[data-pack-update]").forEach((btn) => {
        btn.addEventListener("click", async () => {
          const id = btn.getAttribute("data-pack-update");
          if (!id) return;
          try {
            const updated = await state.client.packs.update(id, {});
            setMeta(updated);
            toast("info", updated?.reason || (updated?.updated ? "Pack updated" : "No update applied"));
          } catch (e) {
            toast("err", `Update failed: ${e instanceof Error ? e.message : String(e)}`);
          }
        });
      });

      packsListEl.querySelectorAll("[data-pack-remove]").forEach((btn) => {
        btn.addEventListener("click", async () => {
          const id = btn.getAttribute("data-pack-remove");
          if (!id) return;
          if (!window.confirm(`Uninstall pack ${id}?`)) return;
          try {
            await state.client.packs.uninstall({ pack_id: id });
            toast("ok", `Uninstalled ${id}`);
            await loadPacks();
          } catch (e) {
            toast("err", `Uninstall failed: ${e instanceof Error ? e.message : String(e)}`);
          }
        });
      });
    } catch (e) {
      packsListEl.innerHTML = `<div class="tcp-card"><p class="text-rose-300 text-sm">Failed to load packs: ${escapeHtml(
        e instanceof Error ? e.message : String(e)
      )}</p></div>`;
      renderIcons();
    }
  };

  byId("packs-refresh-btn")?.addEventListener("click", () => void loadPacks());
  byId("packs-cap-discovery-btn")?.addEventListener("click", async () => {
    try {
      const discovery = await state.client.capabilities.discovery();
      setMeta(discovery);
      const count = Array.isArray(discovery?.tools) ? discovery.tools.length : 0;
      toast("ok", `Discovered ${count} tools`);
    } catch (e) {
      toast("err", `Capability discovery failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });
  byId("packs-install-btn")?.addEventListener("click", async () => {
    const url = String(byId("packs-install-url")?.value || "").trim();
    const path = String(byId("packs-install-path")?.value || "").trim();
    if (!url && !path) {
      toast("err", "Provide either URL or server path.");
      return;
    }
    try {
      const payload = await state.client.packs.install({
        url: url || undefined,
        path: path || undefined,
        source: { kind: "control_panel" },
      });
      setMeta(payload);
      toast(
        "ok",
        `Installed ${payload?.installed?.name || "pack"} ${payload?.installed?.version || ""}`.trim()
      );
      await loadPacks();
    } catch (e) {
      toast("err", `Install failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });

  await loadPacks();
}
