import { renderChannels } from "./channels.js";
import { renderFiles } from "./files.js";
import { renderMcp } from "./mcp.js";
import { renderPacks } from "./packs.js";

const CUSTOM_PROVIDER_VALUE = "__custom_provider__";
const SETTINGS_TABS = ["general", "packs", "channels", "mcp", "files"];

function parseSettingsTabFromHash() {
  const hash = String(window.location.hash || "");
  const [, rawQuery = ""] = hash.split("?");
  const params = new URLSearchParams(rawQuery);
  const tab = String(params.get("tab") || "general")
    .trim()
    .toLowerCase();
  return SETTINGS_TABS.includes(tab) ? tab : "general";
}

function writeSettingsTabToHash(tab) {
  const next = SETTINGS_TABS.includes(String(tab || "").toLowerCase())
    ? String(tab).toLowerCase()
    : "general";
  const params = new URLSearchParams();
  params.set("tab", next);
  const nextHash = `#/settings?${params.toString()}`;
  if (window.location.hash !== nextHash) window.location.hash = nextHash;
}

export async function renderSettings(ctx) {
  const { byId, state, escapeHtml, renderIcons, THEMES = [], setTheme } = ctx;
  const settingsRoot = byId("view");
  const providerLocked = !state.providerReady;
  const requestedTab = parseSettingsTabFromHash();
  const activeTab = providerLocked ? "general" : requestedTab;
  if (providerLocked && requestedTab !== "general" && window.location.hash !== "#/settings?tab=general") {
    window.location.hash = "#/settings?tab=general";
  }
  settingsRoot.innerHTML = `
    <div class="tcp-card">
      <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h3 class="tcp-title flex items-center gap-2"><i data-lucide="sliders-horizontal"></i> Settings</h3>
        <span class="tcp-badge-info">Unified Surface</span>
      </div>
      <p class="tcp-subtle mb-3">Configure platform behavior, integrations, and assets without leaving Settings.</p>
      ${
        providerLocked
          ? `<div class="mb-3 rounded-xl border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm text-amber-200">
               Complete provider + default model setup first. Other sections stay locked until this wizard is finished.
             </div>`
          : ""
      }
      <div class="tcp-settings-tabs" role="tablist" aria-label="Settings sections">
        <button class="tcp-settings-tab tcp-settings-tab-underline ${activeTab === "general" ? "active" : ""}" data-settings-tab="general" role="tab" aria-selected="${activeTab === "general"}"><i data-lucide="settings-2"></i> General</button>
        <button class="tcp-settings-tab tcp-settings-tab-underline ${activeTab === "packs" ? "active" : ""} ${providerLocked ? "locked" : ""}" data-settings-tab="packs" role="tab" aria-selected="${activeTab === "packs"}" ${providerLocked ? 'disabled aria-disabled="true"' : ""}><i data-lucide="package"></i> Packs</button>
        <button class="tcp-settings-tab tcp-settings-tab-underline ${activeTab === "channels" ? "active" : ""} ${providerLocked ? "locked" : ""}" data-settings-tab="channels" role="tab" aria-selected="${activeTab === "channels"}" ${providerLocked ? 'disabled aria-disabled="true"' : ""}><i data-lucide="message-circle"></i> Channels</button>
        <button class="tcp-settings-tab tcp-settings-tab-underline ${activeTab === "mcp" ? "active" : ""} ${providerLocked ? "locked" : ""}" data-settings-tab="mcp" role="tab" aria-selected="${activeTab === "mcp"}" ${providerLocked ? 'disabled aria-disabled="true"' : ""}><i data-lucide="link"></i> MCP</button>
        <button class="tcp-settings-tab tcp-settings-tab-underline ${activeTab === "files" ? "active" : ""} ${providerLocked ? "locked" : ""}" data-settings-tab="files" role="tab" aria-selected="${activeTab === "files"}" ${providerLocked ? 'disabled aria-disabled="true"' : ""}><i data-lucide="folder-open"></i> Files</button>
      </div>
      <div class="mt-4 border-t border-slate-800 pt-4">
        <div id="settings-tab-content" class="grid gap-4"></div>
      </div>
    </div>
  `;

  settingsRoot
    .querySelectorAll("[data-settings-tab]")
    .forEach((btn) =>
      btn.addEventListener("click", () => {
        if (btn.disabled) return;
        const tab = String(btn.getAttribute("data-settings-tab") || "").trim().toLowerCase();
        writeSettingsTabToHash(tab);
      })
    );

  const content = byId("settings-tab-content");
  if (!content) return;

  if (activeTab === "general") {
    content.innerHTML = `
      <div class="tcp-card">
        <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
          <h3 class="tcp-title flex items-center gap-2"><i data-lucide="sparkles"></i> Appearance</h3>
          <span class="tcp-badge-info">Theme Selector</span>
        </div>
        <p class="tcp-subtle mb-3">Pick the control-panel theme. Changes apply instantly across all pages.</p>
        <div class="grid gap-3 md:grid-cols-2">
          <div>
            <label class="mb-1 block text-sm text-slate-300">Theme</label>
            <select id="settings-theme-id" class="tcp-select">
              ${THEMES.map((theme) => `<option value="${escapeHtml(theme.id)}" ${theme.id === state.themeId ? "selected" : ""}>${escapeHtml(theme.name)}</option>`).join("")}
            </select>
          </div>
        </div>
        <div class="theme-swatch-grid mt-3">
          ${THEMES.map(
            (theme) => `
            <button class="theme-swatch ${theme.id === state.themeId ? "active" : ""}" data-theme-select="${escapeHtml(theme.id)}" type="button">
              <span class="theme-swatch-dot" style="background:${escapeHtml(theme.cssVars["--color-primary"] || "#888")}"></span>
              <span>${escapeHtml(theme.name)}</span>
            </button>
          `
          ).join("")}
        </div>
      </div>
      <div class="tcp-card">
        <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
          <h3 class="tcp-title flex items-center gap-2"><i data-lucide="settings-2"></i> Provider Setup Wizard</h3>
          <span class="${state.providerReady ? "tcp-badge-ok" : "tcp-badge-warn"}">${state.providerReady ? "Ready" : "Not Configured"}</span>
        </div>
        <p class="tcp-subtle">Step 1: Select provider. Step 2: Configure model. Step 3: Add key (if required). Step 4: Run test. Step 5: Set bot identity + personality.</p>
        <div class="mt-3 flex flex-wrap gap-2">
          <span class="${state.providerDefault ? "tcp-badge-ok" : "tcp-badge-warn"}">Default: ${escapeHtml(state.providerDefault || "none")}</span>
          <span class="${state.providerConnected.length > 0 ? "tcp-badge-info" : "tcp-badge-warn"}">Connected: ${state.providerConnected.length}</span>
        </div>
        ${state.providerError ? `<p class="mt-3 rounded-xl border border-amber-700/60 bg-amber-950/30 px-3 py-2 text-sm text-amber-300"><i data-lucide="triangle-alert"></i> ${escapeHtml(state.providerError)}</p>` : ""}
        <div id="provider-settings" class="mt-4"></div>
        <div id="identity-settings" class="mt-6 border-t border-slate-800 pt-5"></div>
      </div>
      <div class="tcp-card">
        <h3 class="tcp-title mb-2 flex items-center gap-2"><i data-lucide="shield"></i> Session Authorization</h3>
        <p class="tcp-subtle">Use Logout in the sidebar to clear your current portal session token binding.</p>
      </div>
    `;
    const applyThemeSelection = (id) => {
      const nextId = String(id || "").trim();
      if (!nextId || !setTheme) return;
      setTheme(nextId);
      renderSettings(ctx);
    };
    byId("settings-theme-id")?.addEventListener("change", (e) =>
      applyThemeSelection(e?.target?.value)
    );
    settingsRoot.querySelectorAll("[data-theme-select]").forEach((btn) =>
      btn.addEventListener("click", () =>
        applyThemeSelection(btn.getAttribute("data-theme-select"))
      )
    );
    await renderProvidersBlock(ctx, byId("provider-settings"));
    await renderIdentityBlock(ctx, byId("identity-settings"));
    renderIcons(settingsRoot);
    return;
  }

  content.innerHTML = `<div id="settings-subview-host" class="grid gap-4"></div>`;
  const host = byId("settings-subview-host");
  if (!host) return;
  const scopedCtx = {
    ...ctx,
    embeddedInSettings: true,
    byId: (id) => {
      if (id === "view") return host;
      return host.querySelector(`#${id}`);
    },
  };
  if (activeTab === "packs") await renderPacks(scopedCtx);
  else if (activeTab === "channels") await renderChannels(scopedCtx);
  else if (activeTab === "mcp") await renderMcp(scopedCtx);
  else if (activeTab === "files") await renderFiles(scopedCtx);
  // Re-hydrate icons for the full settings surface so tab icons remain visible
  // after switching away from General.
  renderIcons(settingsRoot);
}

async function renderProvidersBlock(ctx, container) {
  const { state, api, toast, escapeHtml, providerHints, refreshProviderStatus, renderIcons } = ctx;
  const catalog = await state.client.providers.catalog();
  const config = await state.client.providers.config();
  let authStatusRaw = await state.client.providers.authStatus().catch(() => ({}));

  const catalogProviders = catalog.all || [];
  const providerIds = new Set(catalogProviders.map((p) => p.id));
  const defaultProvider = String(
    catalog.default || config.default || catalogProviders[0]?.id || ""
  ).trim();

  let selectedProvider = providerIds.has(defaultProvider) ? defaultProvider : CUSTOM_PROVIDER_VALUE;
  let selectedModel = "";
  let customProviderId = selectedProvider === CUSTOM_PROVIDER_VALUE ? defaultProvider : "";
  let customProviderUrl = customProviderId
    ? String(config.providers?.[customProviderId]?.url || "")
    : "";
  let customProviderModel = customProviderId
    ? String(
        config.providers?.[customProviderId]?.defaultModel ||
          config.providers?.[customProviderId]?.default_model ||
          ""
      )
    : "";
  let replaceStoredKey = false;

  const syncModel = () => {
    if (selectedProvider === CUSTOM_PROVIDER_VALUE) return;
    const entry = catalogProviders.find((p) => p.id === selectedProvider);
    const models = Object.keys(entry?.models || {});
    const cfg = config.providers?.[selectedProvider] || {};
    selectedModel = cfg.defaultModel || cfg.default_model || models[0] || "";
  };
  syncModel();

  const providerOptions = [
    ...catalogProviders.map((p) => ({
      id: p.id,
      label: providerHints[p.id]?.label || p.name || p.id,
    })),
    { id: CUSTOM_PROVIDER_VALUE, label: "Custom Provider" },
  ];

  const normalizeProviderId = (providerId) => String(providerId || "").trim().toLowerCase();
  const resolveHasStoredKey = (providerId) => {
    const id = normalizeProviderId(providerId);
    if (!id) return false;
    const status = authStatusRaw;
    if (status && typeof status === "object") {
      const direct = status[id];
      if (direct && typeof direct === "object") {
        if (direct.has_key === true || direct.hasKey === true) return true;
        if (direct.configured === true || direct.connected === true) return true;
      }
      if (status.providers && typeof status.providers === "object") {
        const nested = status.providers[id];
        if (nested && typeof nested === "object") {
          if (nested.has_key === true || nested.hasKey === true) return true;
          if (nested.configured === true || nested.connected === true) return true;
        }
      }
    }
    return false;
  };
  const providerNeedsApiKey = (providerId) => {
    const id = normalizeProviderId(providerId);
    return !!id && id !== "ollama" && id !== "local";
  };

  const render = () => {
    const models =
      selectedProvider === CUSTOM_PROVIDER_VALUE
        ? []
        : Object.keys((catalogProviders.find((p) => p.id === selectedProvider) || {}).models || {});

    const effectiveProviderId =
      selectedProvider === CUSTOM_PROVIDER_VALUE ? customProviderId : selectedProvider;
    const hasStoredKey = resolveHasStoredKey(effectiveProviderId);
    const allowKeyEntry = !hasStoredKey || replaceStoredKey;
    container.innerHTML = `
      <div class="grid gap-3 md:grid-cols-2">
        <div>
          <label class="mb-1 block text-sm text-slate-300">Provider</label>
          <select id="provider-select" class="tcp-select">
            ${providerOptions
              .map(
                (o) =>
                  `<option value="${escapeHtml(o.id)}" ${o.id === selectedProvider ? "selected" : ""}>${escapeHtml(o.label)}</option>`
              )
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
          <input id="provider-key" class="tcp-input" type="password" placeholder="${escapeHtml(providerHints[selectedProvider]?.placeholder || "sk-...")}" ${allowKeyEntry ? "" : "disabled"} />
          <p class="mt-1 text-xs ${hasStoredKey ? "text-lime-300" : "text-amber-300"}">
            ${hasStoredKey ? "A key is already stored for this provider. Leave blank to keep it." : "No stored key detected for this provider."}
          </p>
          ${
            hasStoredKey
              ? `<div class="mt-2 flex flex-wrap gap-2">
                   <button id="provider-toggle-replace-key" class="tcp-btn h-8 px-2.5 text-xs">${replaceStoredKey ? "Keep existing key" : "Replace key"}</button>
                   <button id="provider-clear-key" class="tcp-btn-danger h-8 px-2.5 text-xs">Clear stored key</button>
                 </div>`
              : ""
          }
        </div>
        <div class="flex items-end justify-end gap-2">
          <button id="provider-test" class="tcp-btn"><i data-lucide="flask-conical"></i> Test Model Run</button>
          <button id="provider-save" class="tcp-btn-primary"><i data-lucide="save"></i> Save Provider</button>
        </div>
      </div>
      <div id="provider-test-status" class="mt-2 text-xs tcp-subtle"></div>
    `;

    container.querySelector("#provider-select").addEventListener("change", (e) => {
      selectedProvider = e.target.value;
      if (selectedProvider !== CUSTOM_PROVIDER_VALUE) syncModel();
      render();
    });

    const modelEl = container.querySelector("#provider-model");
    if (modelEl) modelEl.addEventListener("change", (e) => (selectedModel = e.target.value));

    const customIdEl = container.querySelector("#custom-provider-id");
    if (customIdEl)
      customIdEl.addEventListener("input", (e) => (customProviderId = e.target.value.trim()));

    const customUrlEl = container.querySelector("#custom-provider-url");
    if (customUrlEl)
      customUrlEl.addEventListener("input", (e) => (customProviderUrl = e.target.value.trim()));

    const customModelEl = container.querySelector("#custom-provider-model");
    if (customModelEl)
      customModelEl.addEventListener("input", (e) => (customProviderModel = e.target.value.trim()));

    const replaceToggleEl = container.querySelector("#provider-toggle-replace-key");
    if (replaceToggleEl) {
      replaceToggleEl.addEventListener("click", () => {
        replaceStoredKey = !replaceStoredKey;
        render();
      });
    }

    const clearKeyEl = container.querySelector("#provider-clear-key");
    if (clearKeyEl) {
      clearKeyEl.addEventListener("click", async () => {
        const providerId = normalizeProviderId(
          selectedProvider === CUSTOM_PROVIDER_VALUE ? customProviderId : selectedProvider
        );
        if (!providerId) {
          toast("err", "Select a provider first.");
          return;
        }
        try {
          await api(`/api/engine/auth/${encodeURIComponent(providerId)}`, { method: "DELETE" });
          authStatusRaw = await state.client.providers.authStatus().catch(() => authStatusRaw);
          replaceStoredKey = true;
          toast("ok", `Cleared stored key for ${providerId}.`);
          render();
        } catch (e) {
          toast("err", e instanceof Error ? e.message : String(e));
        }
      });
    }

    const persistProviderConfig = async ({ quiet = false } = {}) => {
      const key = container.querySelector("#provider-key")?.value?.trim?.() || "";
      const targetProviderId = normalizeProviderId(
        selectedProvider === CUSTOM_PROVIDER_VALUE ? customProviderId : selectedProvider
      );
      const hadStoredKey = resolveHasStoredKey(targetProviderId);
      const shouldWriteKey = key.length > 0;
      if (selectedProvider === CUSTOM_PROVIDER_VALUE) {
        const normalizedCustomProviderId = normalizeProviderId(customProviderId);
        if (!normalizedCustomProviderId) throw new Error("Custom provider ID is required.");
        if (!customProviderUrl) throw new Error("Custom provider URL is required.");
        if (!customProviderModel) throw new Error("Custom default model is required.");

        await api("/api/engine/config", {
          method: "PATCH",
          body: JSON.stringify({
            default_provider: normalizedCustomProviderId,
            providers: {
              [normalizedCustomProviderId]: {
                url: customProviderUrl,
                default_model: customProviderModel,
              },
            },
          }),
        });

        if (hadStoredKey && replaceStoredKey && !key) {
          throw new Error("Enter a new API key or keep existing key.");
        }
        if (shouldWriteKey) await state.client.providers.setApiKey(normalizedCustomProviderId, key);
      } else {
        if (!selectedProvider) throw new Error("Select a provider first.");
        if (!selectedModel) throw new Error("Select a default model first.");
        if (hadStoredKey && replaceStoredKey && !key) {
          throw new Error("Enter a new API key or keep existing key.");
        }
        if (shouldWriteKey) await state.client.providers.setApiKey(selectedProvider, key);
        await state.client.providers.setDefaults(selectedProvider, selectedModel);
      }

      await refreshProviderStatus();
      if (shouldWriteKey) {
        authStatusRaw = await state.client.providers.authStatus().catch(() => authStatusRaw);
        replaceStoredKey = false;
      }
      if (!quiet) toast("ok", "Provider configuration saved.");
    };

    container.querySelector("#provider-save").addEventListener("click", async () => {
      const saveBtn = container.querySelector("#provider-save");
      try {
        saveBtn.disabled = true;
        await persistProviderConfig();
        renderSettings(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      } finally {
        saveBtn.disabled = false;
      }
    });

    container.querySelector("#provider-test").addEventListener("click", async () => {
      const testBtn = container.querySelector("#provider-test");
      const statusEl = container.querySelector("#provider-test-status");
      const originalLabel = testBtn.innerHTML;
      const TEST_TIMEOUT_MS = 120000;
      const waitForRunToSettle = async (sessionId, targetRunId, timeoutMs) => {
        const startedAt = Date.now();
        while (Date.now() - startedAt < timeoutMs) {
          const runState = await state.client.sessions
            .activeRun(sessionId)
            .catch(() => ({ active: null }));
          const activeRunId = String(runState?.active?.runId || "").trim();
          if (!activeRunId || (targetRunId && activeRunId !== targetRunId)) return true;
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
        return false;
      };
      try {
        testBtn.disabled = true;
        testBtn.innerHTML = '<i data-lucide="flask-conical"></i> Testing...';
        renderIcons?.(container);
        if (statusEl) {
          statusEl.className = "mt-2 text-xs tcp-subtle";
          statusEl.textContent = "Saving provider settings and running a test request...";
        }

        await persistProviderConfig({ quiet: true });
        const runProviderId = normalizeProviderId(
          selectedProvider === CUSTOM_PROVIDER_VALUE ? customProviderId : selectedProvider
        );
        const runModelId = String(
          selectedProvider === CUSTOM_PROVIDER_VALUE ? customProviderModel : selectedModel
        ).trim();
        if (!runProviderId) throw new Error("Select a provider before running the model test.");
        if (!runModelId) throw new Error("Select a default model before running the model test.");
        if (providerNeedsApiKey(runProviderId) && !resolveHasStoredKey(runProviderId)) {
          throw new Error("No stored key detected for this provider. Save a valid API key first.");
        }

        const runSingleTest = async () => {
          const sid = await state.client.sessions.create({
            title: `Provider test ${new Date().toISOString()}`,
            provider: runProviderId,
            model: runModelId,
          });
          try {
            // Use one async run path only. Mixing promptSync + promptAsync can
            // leave a long-running sync run in-flight and make the test look stuck.
            const { runId } = await state.client.sessions.promptAsync(
              sid,
              "Provider connectivity test: reply with READY.",
              { provider: runProviderId, model: runModelId }
            );
            if (statusEl) {
              statusEl.className = "mt-2 text-xs tcp-subtle";
              statusEl.textContent = "Run accepted. Waiting for provider response...";
            }
            const settled = await waitForRunToSettle(sid, String(runId || "").trim(), TEST_TIMEOUT_MS);
            if (!settled) {
              throw new Error("Model test timed out waiting for run completion.");
            }
            const runEvents = await state.client.runEvents(String(runId || "").trim(), {
              tail: 80,
            }).catch(() => []);
            const providerErrorEvent = runEvents.find(
              (ev) => String(ev?.type || "").trim() === "provider.call.error"
            );
            if (providerErrorEvent) {
              const props = providerErrorEvent?.properties || {};
              const detail = String(props?.detail || props?.error || "").trim();
              const code = String(props?.error_code || "").trim();
              throw new Error(
                `Provider call failed${code ? ` (${code})` : ""}${detail ? `: ${detail}` : ""}`
              );
            }
            const messages = await state.client.sessions.messages(sid).catch(() => []);
            const assistantText = messages
              .map((m) => (m.parts || []).map((p) => String(p?.text || "")).join("\n"))
              .join("\n")
              .trim();
            if (!assistantText) {
              const eventTypes = runEvents.map((ev) => String(ev?.type || "").trim()).filter(Boolean);
              const summary = eventTypes.length
                ? ` Events: ${eventTypes.slice(-8).join(", ")}`
                : "";
              throw new Error(`Run finished but no assistant response was returned.${summary}`);
            }
            return assistantText;
          } finally {
            await state.client.sessions.delete(sid).catch(() => {});
          }
        };

        let assistantText = "";
        try {
          assistantText = await runSingleTest();
        } catch (firstErr) {
          const firstMessage = firstErr instanceof Error ? firstErr.message : String(firstErr);
          if (/BodyStreamBuffer was aborted/i.test(firstMessage)) {
            if (statusEl) {
              statusEl.className = "mt-2 text-xs text-amber-300";
              statusEl.textContent = "Provider stream aborted once. Retrying test...";
            }
            assistantText = await runSingleTest();
          } else {
            throw firstErr;
          }
        }

        await refreshProviderStatus();
        if (statusEl) {
          statusEl.className = "mt-2 text-xs text-lime-300";
          const normalized = assistantText.toUpperCase();
          statusEl.textContent = normalized.includes("READY")
            ? "Model test succeeded."
            : `Model responded successfully: ${assistantText.slice(0, 140)}`;
        }
        toast("ok", "Model run test completed.");
      } catch (e) {
        const rawMessage = e instanceof Error ? e.message : String(e);
        const message = /BodyStreamBuffer was aborted/i.test(rawMessage)
          ? "Provider stream was aborted. Check provider credentials/network, then retry."
          : rawMessage;
        if (statusEl) {
          statusEl.className = "mt-2 text-xs text-rose-300";
          statusEl.textContent = message;
        }
        toast("err", message);
      } finally {
        testBtn.disabled = false;
        testBtn.innerHTML = originalLabel;
        renderIcons?.(container);
      }
    });

    renderIcons?.(container);
  };

  render();
}

async function renderIdentityBlock(ctx, container) {
  const { state, toast, escapeHtml, refreshIdentityStatus, renderShell, renderIcons, api } = ctx;
  const getIdentity = async () => {
    if (state.client?.identity?.get) {
      return state.client.identity.get();
    }
    return api("/api/engine/config/identity", { method: "GET" });
  };
  const patchIdentity = async (payload) => {
    if (state.client?.identity?.patch) {
      return state.client.identity.patch(payload);
    }
    return api("/api/engine/config/identity", {
      method: "PATCH",
      body: JSON.stringify(payload),
    });
  };
  let payload;
  try {
    payload = await getIdentity();
  } catch (e) {
    container.innerHTML = `
      <p class="rounded-xl border border-rose-700/60 bg-rose-950/30 px-3 py-2 text-sm text-rose-300">
        Failed to load identity settings: ${escapeHtml(e instanceof Error ? e.message : String(e))}
      </p>
    `;
    return;
  }

  const identity = payload?.identity || {};
  const bot = identity?.bot || {};
  const aliases = bot?.aliases || {};
  const personality = identity?.personality || {};
  const defaults = personality?.default || {};
  const presets =
    Array.isArray(payload?.presets) && payload.presets.length > 0
      ? payload.presets
      : [
          { id: "balanced", label: "Balanced" },
          { id: "concise", label: "Concise" },
          { id: "friendly", label: "Friendly" },
          { id: "mentor", label: "Mentor" },
          { id: "critical", label: "Critical" },
        ];

  let canonicalName = String(bot?.canonical_name || bot?.canonicalName || "").trim();
  let avatarUrl = String(bot?.avatar_url || bot?.avatarUrl || "").trim();
  let controlPanelAlias = String(aliases?.control_panel || aliases?.controlPanel || "").trim();
  let preset = String(defaults?.preset || "balanced").trim() || "balanced";
  let customInstructions = String(
    defaults?.custom_instructions || defaults?.customInstructions || ""
  ).trim();

  const render = () => {
    container.innerHTML = `
      <div class="mb-3">
        <h4 class="tcp-title flex items-center gap-2"><i data-lucide="bot"></i> Identity & Personality</h4>
        <p class="tcp-subtle mt-1">Control assistant naming and default response style.</p>
      </div>

      <div class="grid gap-3 md:grid-cols-2">
        <div>
          <label class="mb-1 block text-sm text-slate-300">Canonical bot name</label>
          <input id="identity-canonical-name" class="tcp-input" placeholder="Assistant" value="${escapeHtml(canonicalName)}" />
        </div>
        <div>
          <label class="mb-1 block text-sm text-slate-300">Control panel alias (optional)</label>
          <input id="identity-control-panel-alias" class="tcp-input" placeholder="Assistant Control Panel" value="${escapeHtml(controlPanelAlias)}" />
        </div>
      </div>

      <div class="mt-3">
        <label class="mb-1 block text-sm text-slate-300">Avatar (optional)</label>
        <div class="flex flex-wrap items-center gap-3">
          <div class="h-10 w-10 overflow-hidden rounded-xl border border-slate-600 bg-muted">
            <img src="${escapeHtml(avatarUrl || "/tandem-logo.png")}" alt="${escapeHtml(canonicalName || "Assistant")}" class="h-full w-full object-cover" />
          </div>
          <input id="identity-avatar-file" type="file" accept="image/png,image/jpeg,image/webp,image/gif" class="tcp-input !h-auto !py-1.5 text-xs" />
          ${
            avatarUrl
              ? '<button id="identity-avatar-clear" class="tcp-btn h-8 px-2 text-xs">Remove</button>'
              : ""
          }
        </div>
      </div>

      <div class="mt-3 grid gap-3 md:grid-cols-2">
        <div>
          <label class="mb-1 block text-sm text-slate-300">Personality preset</label>
          <select id="identity-preset" class="tcp-select">
            ${presets
              .map(
                (entry) =>
                  `<option value="${escapeHtml(entry.id)}" ${entry.id === preset ? "selected" : ""}>${escapeHtml(entry.label || entry.id)}</option>`
              )
              .join("")}
          </select>
        </div>
      </div>

      <div class="mt-3">
        <label class="mb-1 block text-sm text-slate-300">Custom personality instructions (optional)</label>
        <textarea id="identity-custom-instructions" class="tcp-input min-h-[96px]" rows="4" placeholder="Keep responses concise and operationally focused.">${escapeHtml(customInstructions)}</textarea>
      </div>

      <div class="mt-3 flex items-center justify-end gap-2">
        <button id="identity-save" class="tcp-btn-primary"><i data-lucide="save"></i> Save Identity</button>
      </div>
    `;

    container.querySelector("#identity-canonical-name").addEventListener("input", (e) => {
      canonicalName = e.target.value;
    });
    container.querySelector("#identity-control-panel-alias").addEventListener("input", (e) => {
      controlPanelAlias = e.target.value;
    });
    const avatarFileEl = container.querySelector("#identity-avatar-file");
    if (avatarFileEl) {
      avatarFileEl.addEventListener("change", async (e) => {
        const file = e.target?.files?.[0];
        if (!file) return;
        if (file.size > 10 * 1024 * 1024) {
          toast("err", "Avatar image is too large (max 10 MB).");
          return;
        }
        const reader = new FileReader();
        reader.onload = () => {
          const value = typeof reader.result === "string" ? reader.result : "";
          if (!value) {
            toast("err", "Failed to read avatar file.");
            return;
          }
          avatarUrl = value;
          render();
        };
        reader.onerror = () => toast("err", "Failed to read avatar file.");
        reader.readAsDataURL(file);
      });
    }
    const avatarClearEl = container.querySelector("#identity-avatar-clear");
    if (avatarClearEl) {
      avatarClearEl.addEventListener("click", () => {
        avatarUrl = "";
        render();
      });
    }
    container.querySelector("#identity-preset").addEventListener("change", (e) => {
      preset = e.target.value;
    });
    container.querySelector("#identity-custom-instructions").addEventListener("input", (e) => {
      customInstructions = e.target.value;
    });

    container.querySelector("#identity-save").addEventListener("click", async () => {
      try {
        const nextCanonical = canonicalName.trim();
        if (!nextCanonical) throw new Error("Canonical bot name is required.");
        const nextAlias = controlPanelAlias.trim();
        const nextCustom = customInstructions.trim();

        const updated = await patchIdentity({
          identity: {
            bot: {
              canonical_name: nextCanonical,
              avatar_url: avatarUrl || null,
              aliases: {
                control_panel: nextAlias || undefined,
              },
            },
            personality: {
              default: {
                preset: preset || "balanced",
                custom_instructions: nextCustom || null,
              },
            },
          },
        });

        await refreshIdentityStatus();
        renderShell();
        toast("ok", "Identity settings saved.");
        payload = updated;
        canonicalName = String(updated?.identity?.bot?.canonical_name || "").trim();
        avatarUrl = String(updated?.identity?.bot?.avatar_url || "").trim();
        controlPanelAlias = String(updated?.identity?.bot?.aliases?.control_panel || "").trim();
        preset =
          String(updated?.identity?.personality?.default?.preset || "balanced").trim() ||
          "balanced";
        customInstructions = String(
          updated?.identity?.personality?.default?.custom_instructions || ""
        ).trim();
        render();
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });
    renderIcons?.(container);
  };

  render();
}
