const CUSTOM_PROVIDER_VALUE = "__custom_provider__";

export async function renderSettings(ctx) {
  const { byId, state, escapeHtml } = ctx;
  byId("view").innerHTML = `
    <div class="tcp-card">
      <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h3 class="tcp-title flex items-center gap-2"><i data-lucide="settings-2"></i> Provider Setup Wizard</h3>
        <span class="${state.providerReady ? "tcp-badge-ok" : "tcp-badge-warn"}">${state.providerReady ? "Ready" : "Not Configured"}</span>
      </div>
      <p class="tcp-subtle">Step 1: Select provider. Step 2: Configure model. Step 3: Add key (if required). Step 4: Run test.</p>
      <div class="mt-3 flex flex-wrap gap-2">
        <span class="${state.providerDefault ? "tcp-badge-ok" : "tcp-badge-warn"}">Default: ${escapeHtml(state.providerDefault || "none")}</span>
        <span class="${state.providerConnected.length > 0 ? "tcp-badge-info" : "tcp-badge-warn"}">Connected: ${state.providerConnected.length}</span>
      </div>
      ${state.providerError ? `<p class="mt-3 rounded-xl border border-amber-700/60 bg-amber-950/30 px-3 py-2 text-sm text-amber-300"><i data-lucide="triangle-alert"></i> ${escapeHtml(state.providerError)}</p>` : ""}
      <div id="provider-settings" class="mt-4"></div>
    </div>
    <div class="tcp-card">
      <h3 class="tcp-title mb-2 flex items-center gap-2"><i data-lucide="shield"></i> Session Authorization</h3>
      <p class="tcp-subtle">Use Logout in the sidebar to clear your current portal session token binding.</p>
    </div>
  `;
  await renderProvidersBlock(ctx, byId("provider-settings"));
}

async function renderProvidersBlock(ctx, container) {
  const { state, api, toast, escapeHtml, providerHints, refreshProviderStatus } = ctx;
  const catalog = await state.client.providers.catalog();
  const config = await state.client.providers.config();

  const catalogProviders = catalog.all || [];
  const providerIds = new Set(catalogProviders.map((p) => p.id));
  const defaultProvider = String(catalog.default || config.default || catalogProviders[0]?.id || "").trim();

  let selectedProvider = providerIds.has(defaultProvider) ? defaultProvider : CUSTOM_PROVIDER_VALUE;
  let selectedModel = "";
  let customProviderId = selectedProvider === CUSTOM_PROVIDER_VALUE ? defaultProvider : "";
  let customProviderUrl = customProviderId ? String(config.providers?.[customProviderId]?.url || "") : "";
  let customProviderModel = customProviderId
    ? String(config.providers?.[customProviderId]?.defaultModel || config.providers?.[customProviderId]?.default_model || "")
    : "";

  const syncModel = () => {
    if (selectedProvider === CUSTOM_PROVIDER_VALUE) return;
    const entry = catalogProviders.find((p) => p.id === selectedProvider);
    const models = Object.keys(entry?.models || {});
    const cfg = config.providers?.[selectedProvider] || {};
    selectedModel = cfg.defaultModel || cfg.default_model || models[0] || "";
  };
  syncModel();

  const providerOptions = [
    ...catalogProviders.map((p) => ({ id: p.id, label: providerHints[p.id]?.label || p.name || p.id })),
    { id: CUSTOM_PROVIDER_VALUE, label: "Custom Provider" },
  ];

  const render = () => {
    const models =
      selectedProvider === CUSTOM_PROVIDER_VALUE
        ? []
        : Object.keys((catalogProviders.find((p) => p.id === selectedProvider) || {}).models || {});

    container.innerHTML = `
      <div class="grid gap-3 md:grid-cols-2">
        <div>
          <label class="mb-1 block text-sm text-slate-300">Provider</label>
          <select id="provider-select" class="tcp-select">
            ${providerOptions
              .map((o) => `<option value="${escapeHtml(o.id)}" ${o.id === selectedProvider ? "selected" : ""}>${escapeHtml(o.label)}</option>`)
              .join("")}
          </select>
        </div>
        ${
          selectedProvider === CUSTOM_PROVIDER_VALUE
            ? `<div>
                <label class="mb-1 block text-sm text-slate-300">Custom Provider ID</label>
                <input id="custom-provider-id" class="tcp-input" placeholder="my-provider" value="${escapeHtml(customProviderId)}" />
              </div>`
            : `<div>
                <label class="mb-1 block text-sm text-slate-300">Model</label>
                <select id="provider-model" class="tcp-select">${models.map((m) => `<option ${m === selectedModel ? "selected" : ""}>${escapeHtml(m)}</option>`).join("")}</select>
              </div>`
        }
      </div>

      ${
        selectedProvider === CUSTOM_PROVIDER_VALUE
          ? `<div class="mt-3 grid gap-3 md:grid-cols-2">
              <div>
                <label class="mb-1 block text-sm text-slate-300">Base URL</label>
                <input id="custom-provider-url" class="tcp-input" placeholder="https://api.example.com/v1" value="${escapeHtml(customProviderUrl)}" />
              </div>
              <div>
                <label class="mb-1 block text-sm text-slate-300">Default Model</label>
                <input id="custom-provider-model" class="tcp-input" placeholder="gpt-4o-mini" value="${escapeHtml(customProviderModel)}" />
              </div>
            </div>
            <p class="tcp-subtle mt-2">Custom providers use OpenAI-compatible chat completions endpoints.</p>`
          : ""
      }

      <div class="mt-3 grid gap-3 md:grid-cols-2">
        <div>
          <label class="mb-1 block text-sm text-slate-300">API Key (optional)</label>
          <input id="provider-key" class="tcp-input" type="password" placeholder="${escapeHtml(providerHints[selectedProvider]?.placeholder || "sk-...")}" />
        </div>
        <div class="flex items-end justify-end gap-2">
          <button id="provider-test" class="tcp-btn"><i data-lucide="flask-conical"></i> Test Model Run</button>
          <button id="provider-save" class="tcp-btn-primary"><i data-lucide="save"></i> Save Provider</button>
        </div>
      </div>
    `;

    container.querySelector("#provider-select").addEventListener("change", (e) => {
      selectedProvider = e.target.value;
      if (selectedProvider !== CUSTOM_PROVIDER_VALUE) syncModel();
      render();
    });

    const modelEl = container.querySelector("#provider-model");
    if (modelEl) modelEl.addEventListener("change", (e) => (selectedModel = e.target.value));

    const customIdEl = container.querySelector("#custom-provider-id");
    if (customIdEl) customIdEl.addEventListener("input", (e) => (customProviderId = e.target.value.trim()));

    const customUrlEl = container.querySelector("#custom-provider-url");
    if (customUrlEl) customUrlEl.addEventListener("input", (e) => (customProviderUrl = e.target.value.trim()));

    const customModelEl = container.querySelector("#custom-provider-model");
    if (customModelEl) customModelEl.addEventListener("input", (e) => (customProviderModel = e.target.value.trim()));

    container.querySelector("#provider-save").addEventListener("click", async () => {
      const key = container.querySelector("#provider-key").value.trim();
      try {
        if (selectedProvider === CUSTOM_PROVIDER_VALUE) {
          if (!customProviderId) throw new Error("Custom provider ID is required.");
          if (!customProviderUrl) throw new Error("Custom provider URL is required.");
          if (!customProviderModel) throw new Error("Custom default model is required.");

          await api("/api/engine/config", {
            method: "PATCH",
            body: JSON.stringify({
              default_provider: customProviderId,
              providers: {
                [customProviderId]: {
                  url: customProviderUrl,
                  default_model: customProviderModel,
                },
              },
            }),
          });

          if (key) await state.client.providers.setApiKey(customProviderId, key);
        } else {
          if (key) await state.client.providers.setApiKey(selectedProvider, key);
          await state.client.providers.setDefaults(selectedProvider, selectedModel);
        }

        await refreshProviderStatus();
        toast("ok", "Provider configuration saved.");
        renderSettings(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });

    container.querySelector("#provider-test").addEventListener("click", async () => {
      try {
        const sid = await state.client.sessions.create({ title: `Provider test ${new Date().toISOString()}` });
        const { runId } = await state.client.sessions.promptAsync(sid, "Reply with exactly: READY");
        let sawResponse = false;
        for await (const event of state.client.stream(sid, runId)) {
          if (event.type === "session.response") {
            const delta = String(event.properties?.delta || "").trim();
            if (delta) sawResponse = true;
          }
          if (event.type === "run.complete" || event.type === "run.failed" || event.type === "session.run.finished") break;
        }
        if (!sawResponse) throw new Error("No model tokens received. Save provider + key first, then retry.");
        await refreshProviderStatus();
        toast("ok", "Model run test succeeded.");
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });
  };

  render();
}
