function parseUrl(input) {
  try {
    return new URL(input);
  } catch {
    return null;
  }
}

function normalizeName(raw) {
  const cleaned = String(raw || "")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return cleaned || "mcp-server";
}

function inferNameFromTransport(transport) {
  const url = parseUrl(transport);
  if (!url) return "";
  const host = String(url.hostname || "").toLowerCase();
  if (!host) return "";
  if (host.endsWith("composio.dev")) return "composio";

  const parts = host.split(".").filter(Boolean);
  if (parts.length === 0) return "";
  const preferred = ["backend", "api", "mcp", "www"].includes(parts[0]) ? parts[1] || parts[0] : parts[0];
  return normalizeName(preferred);
}

function isComposioTransport(transport) {
  const url = parseUrl(transport);
  if (!url) return false;
  const host = String(url.hostname || "").toLowerCase();
  return host.endsWith("composio.dev");
}

function normalizeServerRow(input, fallbackName = "") {
  if (!input || typeof input !== "object") return null;
  const row = input;
  const name = String(row.name || fallbackName || "").trim();
  if (!name) return null;
  return {
    name,
    transport: String(row.transport || "").trim(),
    connected: !!row.connected,
    enabled: row.enabled !== false,
    lastError: String(row.last_error || row.lastError || "").trim(),
    headers: row.headers && typeof row.headers === "object" ? row.headers : {},
    toolCache: Array.isArray(row.tool_cache || row.toolCache) ? row.tool_cache || row.toolCache : [],
  };
}

function normalizeServers(raw) {
  if (Array.isArray(raw)) {
    return raw
      .map((entry) => normalizeServerRow(entry))
      .filter(Boolean)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  if (!raw || typeof raw !== "object") return [];
  if (Array.isArray(raw.servers)) {
    return raw.servers
      .map((entry) => normalizeServerRow(entry))
      .filter(Boolean)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  return Object.entries(raw)
    .map(([name, cfg]) =>
      normalizeServerRow(
        cfg && typeof cfg === "object" ? cfg : { transport: String(cfg || "") },
        name
      )
    )
    .filter(Boolean)
    .sort((a, b) => a.name.localeCompare(b.name));
}

function normalizeTools(raw) {
  if (!Array.isArray(raw)) return [];
  return raw
    .map((tool) => {
      if (typeof tool === "string") return tool;
      if (!tool || typeof tool !== "object") return "";
      return String(
        tool.namespaced_name || tool.namespacedName || tool.id || tool.tool_name || tool.toolName || ""
      ).trim();
    })
    .filter(Boolean);
}

function normalizeCatalog(raw) {
  const catalog = raw && typeof raw === "object" ? raw : {};
  const list = Array.isArray(catalog.servers) ? catalog.servers : [];
  return {
    generatedAt: String(catalog.generated_at || "").trim(),
    count: Number.isFinite(Number(catalog.count)) ? Number(catalog.count) : list.length,
    servers: list
      .map((row) => {
        if (!row || typeof row !== "object") return null;
        return {
          slug: String(row.slug || "").trim(),
          name: String(row.name || row.slug || "").trim(),
          description: String(row.description || "").trim(),
          transportUrl: String(row.transport_url || "").trim(),
          serverConfigName: String(row.server_config_name || row.slug || "").trim(),
          documentationUrl: String(row.documentation_url || "").trim(),
          directoryUrl: String(row.directory_url || "").trim(),
          toolCount: Number.isFinite(Number(row.tool_count)) ? Number(row.tool_count) : 0,
          requiresAuth: row.requires_auth !== false,
          requiresSetup: !!row.requires_setup,
        };
      })
      .filter((row) => row && row.slug && row.transportUrl)
      .sort((a, b) => a.name.localeCompare(b.name)),
  };
}

function authPreview(authMode, token, customHeader, transport) {
  const hasToken = !!String(token || "").trim();
  if (!hasToken || authMode === "none") return "No auth header will be sent.";

  if (authMode === "custom") {
    return customHeader ? `Header preview: ${customHeader}: <token>` : "Set a custom header name.";
  }

  if (authMode === "x-api-key") return "Header preview: x-api-key: <token>";
  if (authMode === "bearer") return "Header preview: Authorization: Bearer <token>";

  if (isComposioTransport(transport)) return "Auto mode: selected x-api-key for this endpoint";
  return "Auto mode: using Authorization Bearer token";
}

function buildHeaders({ authMode, token, customHeader, transport }) {
  const rawToken = String(token || "").trim();
  if (!rawToken || authMode === "none") return {};

  if (authMode === "custom") {
    const headerName = String(customHeader || "").trim();
    if (!headerName) throw new Error("Custom header name is required.");
    return { [headerName]: rawToken };
  }

  if (authMode === "x-api-key") return { "x-api-key": rawToken };

  if (authMode === "bearer") {
    const bearerToken = rawToken.replace(/^bearer\s+/i, "").trim();
    return { Authorization: `Bearer ${bearerToken}` };
  }

  if (isComposioTransport(transport)) return { "x-api-key": rawToken };
  const bearerToken = rawToken.replace(/^bearer\s+/i, "").trim();
  return { Authorization: `Bearer ${bearerToken}` };
}

export async function renderMcp(ctx) {
  const { state, byId, api, toast, escapeHtml, setRoute } = ctx;
  const embeddedInSettings = ctx?.embeddedInSettings === true;
  const [serversRaw, toolsRaw, catalogRaw] = await Promise.all([
    state.client.mcp.list().catch(() => ({})),
    state.client.mcp.listTools().catch(() => []),
    api("/api/engine/mcp/catalog", { method: "GET" }).catch(() => null),
  ]);

  const servers = normalizeServers(serversRaw);
  const toolIds = normalizeTools(toolsRaw);
  const catalog = normalizeCatalog(catalogRaw?.catalog || catalogRaw || null);
  const parseCsv = (value) =>
    String(value || "")
      .split(",")
      .map((part) => part.trim())
      .filter(Boolean);
  const configuredServerNames = new Set(servers.map((row) => row.name.toLowerCase()));

  const movedCard = embeddedInSettings
    ? ""
    : `
    <div class="tcp-card">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 class="tcp-title">Moved To Settings</h3>
          <p class="tcp-subtle">MCP connection management is now organized under Settings.</p>
        </div>
        <button id="mcp-open-settings" class="tcp-btn"><i data-lucide="settings"></i> Open Settings</button>
      </div>
    </div>`;
  byId("view").innerHTML = `
    ${movedCard}
    <div class="grid gap-4 xl:grid-cols-[440px_1fr]">
      <div id="mcp-add-card" class="tcp-card">
        <h3 class="tcp-title mb-2">Add MCP Server</h3>
        <p class="tcp-subtle mb-3">Paste your MCP endpoint URL and optional auth token/header.</p>
        <div class="grid gap-3">
          <div>
            <label class="mb-1 block text-sm text-slate-300">Name</label>
            <input id="mcp-name" class="tcp-input" placeholder="mcp-server" />
          </div>
          <div>
            <label class="mb-1 block text-sm text-slate-300">Transport URL</label>
            <input id="mcp-transport" class="tcp-input" placeholder="https://example.com/mcp" />
          </div>
          <div>
            <label class="mb-1 block text-sm text-slate-300">Auth Mode</label>
            <select id="mcp-auth-mode" class="tcp-select">
              <option value="none" selected>No Auth Header</option>
              <option value="auto">Auto (x-api-key for known providers, else Bearer)</option>
              <option value="x-api-key">x-api-key</option>
              <option value="bearer">Authorization Bearer</option>
              <option value="custom">Custom Header</option>
            </select>
          </div>
          <div id="mcp-custom-header-wrap" class="hidden">
            <label class="mb-1 block text-sm text-slate-300">Custom Header Name</label>
            <input id="mcp-custom-header" class="tcp-input" placeholder="X-My-Token" />
          </div>
          <div>
            <label class="mb-1 block text-sm text-slate-300">Token (optional)</label>
            <input id="mcp-token" class="tcp-input" type="password" placeholder="token" />
          </div>
          <p id="mcp-auth-preview" class="tcp-subtle"></p>
          <button id="mcp-add" class="tcp-btn-primary"><i data-lucide="plug-zap"></i> Add + Connect</button>
        </div>
      </div>

      <div class="grid gap-4">
        <div class="tcp-card">
          <div class="mb-2 flex flex-wrap items-center justify-between gap-2">
            <h3 class="tcp-title">Remote MCP Packs (${catalog.count})</h3>
            <span class="tcp-subtle text-xs">${escapeHtml(
              catalog.generatedAt ? `generated ${catalog.generatedAt}` : "catalog unavailable"
            )}</span>
          </div>
          <p class="tcp-subtle mb-3">Anthropic remote MCP examples exported as per-server TOML packs. Use Apply to prefill transport/name.</p>
          <div class="mb-3 grid gap-2 md:grid-cols-[1fr_auto]">
            <input id="mcp-catalog-search" class="tcp-input" placeholder="Search pack name, slug, or URL" />
            <button id="mcp-catalog-refresh" class="tcp-btn"><i data-lucide="refresh-cw"></i> Refresh</button>
          </div>
          <div id="mcp-catalog-list" class="grid gap-2 md:grid-cols-2 2xl:grid-cols-3 max-h-[520px] overflow-auto pr-1"></div>
        </div>

        <div class="tcp-card">
          <div class="mb-3 flex items-center justify-between gap-2">
            <h3 class="tcp-title">Servers (${servers.length})</h3>
            <button id="mcp-refresh-all" class="tcp-btn"><i data-lucide="refresh-cw"></i> Reload</button>
          </div>
          <div id="mcp-servers" class="tcp-list"></div>
        </div>

        <div class="tcp-card">
          <h3 class="tcp-title mb-3">Discovered MCP Tools (${toolIds.length})</h3>
          <pre class="tcp-code max-h-[320px] overflow-auto">${escapeHtml(toolIds.slice(0, 350).join("\n")) || "No tools discovered yet. Connect a server first."}</pre>
        </div>

        <div class="tcp-card">
          <h3 class="tcp-title mb-2">Capability Readiness Check</h3>
          <p class="tcp-subtle mb-3">Validate required capability IDs before creating or running automation templates.</p>
          <div class="grid gap-2 md:grid-cols-[1fr_auto]">
            <input id="mcp-readiness-required" class="tcp-input" placeholder="required capabilities csv (e.g. github.list_issues,github.create_pull_request)" />
            <button id="mcp-readiness-check" class="tcp-btn"><i data-lucide="shield-check"></i> Check</button>
          </div>
          <pre id="mcp-readiness-result" class="tcp-code mt-3 max-h-[260px] overflow-auto">No readiness check yet.</pre>
        </div>
      </div>
    </div>
  `;
  if (!embeddedInSettings) {
    byId("mcp-open-settings")?.addEventListener("click", () => setRoute("settings"));
  }

  const nameEl = byId("mcp-name");
  const transportEl = byId("mcp-transport");
  const tokenEl = byId("mcp-token");
  const authModeEl = byId("mcp-auth-mode");
  const customHeaderWrapEl = byId("mcp-custom-header-wrap");
  const customHeaderEl = byId("mcp-custom-header");
  const authPreviewEl = byId("mcp-auth-preview");
  const addCardEl = byId("mcp-add-card");

  const focusAddForm = () => {
    addCardEl?.scrollIntoView({ behavior: "smooth", block: "start" });
  };

  const refreshAuthUi = () => {
    const mode = authModeEl.value;
    customHeaderWrapEl.classList.toggle("hidden", mode !== "custom");
    authPreviewEl.textContent = authPreview(
      mode,
      tokenEl.value,
      customHeaderEl.value,
      transportEl.value
    );
  };

  const maybeInferName = () => {
    const current = String(nameEl.value || "").trim();
    if (!current || current === "composio" || current === "mcp-server") {
      const inferred = inferNameFromTransport(transportEl.value.trim());
      if (inferred) nameEl.value = inferred;
    }
  };

  const prefillServerForEditing = (row) => {
    if (!row) return;
    nameEl.value = normalizeName(row.name || "");
    transportEl.value = String(row.transport || "").trim();
    const headers = row.headers && typeof row.headers === "object" ? row.headers : {};
    const keys = Object.keys(headers);
    const authHeaderKey = keys.find((k) => String(k).toLowerCase() === "authorization");
    const apiKeyHeaderKey = keys.find((k) => String(k).toLowerCase() === "x-api-key");

    authModeEl.value = "none";
    tokenEl.value = "";
    customHeaderEl.value = "";

    if (apiKeyHeaderKey) {
      authModeEl.value = "x-api-key";
      tokenEl.value = String(headers[apiKeyHeaderKey] || "").trim();
    } else if (authHeaderKey) {
      authModeEl.value = "bearer";
      tokenEl.value = String(headers[authHeaderKey] || "")
        .replace(/^bearer\s+/i, "")
        .trim();
    } else if (keys.length === 1) {
      authModeEl.value = "custom";
      customHeaderEl.value = keys[0];
      tokenEl.value = String(headers[keys[0]] || "").trim();
    } else if (keys.length > 1) {
      authModeEl.value = "custom";
      customHeaderEl.value = keys[0];
      tokenEl.value = String(headers[keys[0]] || "").trim();
      toast(
        "err",
        "Multiple auth headers detected; form loaded first header only. Re-enter remaining headers if needed."
      );
    }

    maybeInferName();
    refreshAuthUi();
    focusAddForm();
  };

  transportEl.addEventListener("input", () => {
    maybeInferName();
    refreshAuthUi();
  });
  tokenEl.addEventListener("input", refreshAuthUi);
  authModeEl.addEventListener("change", refreshAuthUi);
  customHeaderEl.addEventListener("input", refreshAuthUi);
  refreshAuthUi();

  byId("mcp-add").addEventListener("click", async () => {
    const transport = String(transportEl.value || "").trim();
    const name = normalizeName(nameEl.value || inferNameFromTransport(transport));
    const authMode = String(authModeEl.value || "auto");
    const token = tokenEl.value;
    const customHeader = customHeaderEl.value;

    if (!transport) return toast("err", "Transport URL is required.");
    if (!parseUrl(transport) && !transport.startsWith("stdio:")) {
      return toast("err", "Transport must be a valid URL or stdio:* transport.");
    }

    try {
      const headers = buildHeaders({ authMode, token, customHeader, transport });
      const payload = { name, transport, enabled: true };
      if (Object.keys(headers).length) payload.headers = headers;
      await state.client.mcp.add(payload);
      const connectResult = await state.client.mcp.connect(name);
      if (!connectResult?.ok) {
        const snapshot = normalizeServers(await state.client.mcp.list().catch(() => ({})));
        const failed = snapshot.find((row) => row.name === name);
        const detail = failed?.lastError ? ` ${failed.lastError}` : "";
        const composioHint =
          isComposioTransport(transport) &&
          /401|403|unauthorized|forbidden|invalid api key|api key/i.test(detail)
            ? " Composio usually expects `x-api-key` and a valid `user_id` query param."
            : "";
        throw new Error(`Unable to connect MCP server "${name}".${detail}${composioHint}`);
      }
      toast("ok", `MCP "${name}" connected.`);
      await renderMcp(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
      await renderMcp(ctx);
    }
  });

  byId("mcp-refresh-all").addEventListener("click", () => {
    renderMcp(ctx);
  });

  byId("mcp-readiness-check")?.addEventListener("click", async () => {
    try {
      const required = parseCsv(byId("mcp-readiness-required")?.value);
      if (!required.length) {
        toast("err", "Enter at least one required capability.");
        return;
      }
      const payload = state.client?.capabilities?.readiness
        ? await state.client.capabilities.readiness({
            workflow_id: "control-panel-readiness",
            required_capabilities: required,
          })
        : await api("/api/engine/capabilities/readiness", {
            method: "POST",
            body: JSON.stringify({
              workflow_id: "control-panel-readiness",
              required_capabilities: required,
            }),
          });
      byId("mcp-readiness-result").textContent = JSON.stringify(payload?.readiness || payload, null, 2);
      const runnable = !!(payload?.readiness || payload)?.runnable;
      toast("ok", runnable ? "Ready" : "Not ready");
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });

  const catalogListEl = byId("mcp-catalog-list");
  const catalogSearchEl = byId("mcp-catalog-search");
  const catalogRefreshEl = byId("mcp-catalog-refresh");
  const addCatalogServer = async (picked, connectAfterAdd = false) => {
    if (!picked) return;
    const suggestedName = normalizeName(picked.serverConfigName || picked.slug || picked.name);
    const transport = String(picked.transportUrl || "").trim();
    const authMode = String(authModeEl.value || "auto");
    const token = tokenEl.value;
    const customHeader = customHeaderEl.value;

    if (!transport) {
      toast("err", "Catalog entry has no transport URL.");
      return;
    }
    if (!parseUrl(transport) && !transport.startsWith("stdio:")) {
      toast("err", "Catalog transport URL is invalid.");
      return;
    }

    if (picked.requiresAuth && !String(token || "").trim()) {
      nameEl.value = suggestedName;
      transportEl.value = transport;
      maybeInferName();
      refreshAuthUi();
      focusAddForm();
      toast(
        "err",
        connectAfterAdd
          ? "This MCP requires auth. Add token/header first, then Add + Connect."
          : "This MCP requires auth. Configure token/header first."
      );
      return;
    }

    try {
      const headers = buildHeaders({ authMode, token, customHeader, transport });
      const payload = { name: suggestedName, transport, enabled: true };
      if (Object.keys(headers).length) payload.headers = headers;
      await state.client.mcp.add(payload);

      if (connectAfterAdd) {
        const connectResult = await state.client.mcp.connect(suggestedName);
        if (!connectResult?.ok) {
          const snapshot = normalizeServers(await state.client.mcp.list().catch(() => ({})));
          const failed = snapshot.find((row) => row.name === suggestedName);
          const detail = failed?.lastError ? ` ${failed.lastError}` : "";
          throw new Error(`Added "${suggestedName}" but connect failed.${detail}`);
        }
        toast("ok", `MCP "${suggestedName}" added and connected.`);
      } else {
        toast("ok", `MCP "${suggestedName}" added.`);
      }
      await renderMcp(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
      await renderMcp(ctx);
    }
  };
  const openTomlModal = async (picked) => {
    if (!picked?.slug) return;

    const existing = document.querySelector(".tcp-doc-overlay");
    if (existing) existing.remove();

    const overlay = document.createElement("div");
    overlay.className = "tcp-doc-overlay";
    overlay.innerHTML = `
      <div class="tcp-doc-dialog" role="dialog" aria-modal="true" aria-labelledby="tcp-doc-title">
        <div class="tcp-doc-header">
          <h3 id="tcp-doc-title" class="tcp-doc-title">TOML · ${escapeHtml(picked.name || picked.slug)}</h3>
          <div class="tcp-doc-actions">
            <button type="button" class="tcp-btn" data-doc-copy disabled>Copy</button>
            <button type="button" class="tcp-btn" data-doc-close>Close</button>
          </div>
        </div>
        <pre class="tcp-doc-pre" data-doc-body>Loading TOML…</pre>
      </div>
    `;
    document.body.appendChild(overlay);

    const closeBtn = overlay.querySelector("[data-doc-close]");
    const copyBtn = overlay.querySelector("[data-doc-copy]");
    const bodyPre = overlay.querySelector("[data-doc-body]");

    const cleanup = () => {
      document.removeEventListener("keydown", onKeyDown);
      overlay.remove();
    };
    const onKeyDown = (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        cleanup();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    overlay.addEventListener("click", (event) => {
      if (event.target === overlay) cleanup();
    });
    closeBtn?.addEventListener("click", cleanup);
    copyBtn?.addEventListener("click", async () => {
      try {
        const text = String(bodyPre?.textContent || "");
        if (!text || text.startsWith("Failed to load")) return;
        await navigator.clipboard.writeText(text);
        toast("ok", "TOML copied.");
      } catch {
        toast("err", "Failed to copy TOML.");
      }
    });

    try {
      const res = await fetch(`/api/engine/mcp/catalog/${encodeURIComponent(picked.slug)}/toml`, {
        method: "GET",
        credentials: "include",
        headers: { Accept: "application/toml,text/plain;q=0.9,*/*;q=0.8" },
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const toml = await res.text();
      if (bodyPre) bodyPre.textContent = toml || "Empty TOML response.";
      if (copyBtn) copyBtn.disabled = !toml;
    } catch (e) {
      if (bodyPre) {
        bodyPre.textContent = `Failed to load TOML for ${picked.slug}: ${
          e instanceof Error ? e.message : String(e)
        }`;
      }
    }
  };

  const renderCatalog = () => {
    const query = String(catalogSearchEl?.value || "")
      .trim()
      .toLowerCase();
    const visible = catalog.servers
      .filter((row) => {
        if (!query) return true;
        return (
          row.name.toLowerCase().includes(query) ||
          row.slug.toLowerCase().includes(query) ||
          row.transportUrl.toLowerCase().includes(query)
        );
      })
      .slice(0, 50);

    if (!visible.length) {
      catalogListEl.innerHTML = '<p class="tcp-subtle">No catalog entries match your search.</p>';
      return;
    }

    catalogListEl.innerHTML = visible
      .map(
        (row) => {
          const alreadyConfigured = configuredServerNames.has(
            String(row.serverConfigName || row.slug || "").toLowerCase()
          );
          return `
        <div class="tcp-list-item grid h-full content-start gap-2">
          <div class="flex flex-wrap items-start justify-between gap-2">
            <div>
              <div class="font-semibold">${escapeHtml(row.name)}</div>
              <div class="tcp-subtle text-xs">${escapeHtml(row.slug)}${row.requiresSetup ? " · setup required" : ""}</div>
            </div>
            <div class="flex flex-wrap gap-2">
              <span class="tcp-badge-info">Tools: ${row.toolCount}</span>
              <span class="${row.requiresAuth ? "tcp-badge-warn" : "tcp-badge-ok"}">${row.requiresAuth ? "Auth" : "Authless"}</span>
            </div>
          </div>
          <div class="tcp-subtle text-xs break-all line-clamp-2">${escapeHtml(row.transportUrl)}</div>
          ${row.description ? `<div class="text-xs text-slate-200 line-clamp-2">${escapeHtml(row.description)}</div>` : ""}
          <div class="mt-auto flex flex-wrap gap-2">
            <button class="tcp-btn" data-catalog-apply="${escapeHtml(row.slug)}">Apply</button>
            <button class="tcp-btn" data-catalog-add="${escapeHtml(row.slug)}" ${alreadyConfigured ? "disabled" : ""}>${alreadyConfigured ? "Added" : "Add"}</button>
            <button class="tcp-btn-primary" data-catalog-add-connect="${escapeHtml(row.slug)}" ${alreadyConfigured ? "disabled" : ""}>${alreadyConfigured ? "Added" : "Add + Connect"}</button>
            <button class="tcp-btn" data-catalog-toml="${escapeHtml(row.slug)}">View TOML</button>
            ${
              row.documentationUrl
                ? `<a class="tcp-btn" href="${escapeHtml(row.documentationUrl)}" target="_blank" rel="noreferrer">Docs</a>`
                : ""
            }
          </div>
        </div>
      `
        }
      )
      .join("");

    catalogListEl.querySelectorAll("[data-catalog-apply]").forEach((button) => {
      button.addEventListener("click", () => {
        const slug = String(button.getAttribute("data-catalog-apply") || "").trim();
        const picked = catalog.servers.find((row) => row.slug === slug);
        if (!picked) return;
        nameEl.value = normalizeName(picked.serverConfigName || picked.slug || picked.name);
        transportEl.value = picked.transportUrl;
        maybeInferName();
        refreshAuthUi();
        toast("ok", `Loaded pack ${picked.name}. Add + Connect when ready.`);
      });
    });
    catalogListEl.querySelectorAll("[data-catalog-add]").forEach((button) => {
      button.addEventListener("click", () => {
        const slug = String(button.getAttribute("data-catalog-add") || "").trim();
        const picked = catalog.servers.find((row) => row.slug === slug);
        addCatalogServer(picked, false);
      });
    });
    catalogListEl.querySelectorAll("[data-catalog-add-connect]").forEach((button) => {
      button.addEventListener("click", () => {
        const slug = String(button.getAttribute("data-catalog-add-connect") || "").trim();
        const picked = catalog.servers.find((row) => row.slug === slug);
        addCatalogServer(picked, true);
      });
    });
    catalogListEl.querySelectorAll("[data-catalog-toml]").forEach((button) => {
      button.addEventListener("click", () => {
        const slug = String(button.getAttribute("data-catalog-toml") || "").trim();
        const picked = catalog.servers.find((row) => row.slug === slug);
        openTomlModal(picked);
      });
    });
  };
  catalogSearchEl?.addEventListener("input", renderCatalog);
  catalogRefreshEl?.addEventListener("click", () => {
    renderMcp(ctx);
  });
  renderCatalog();

  const listEl = byId("mcp-servers");
  listEl.innerHTML =
    servers
      .map((server) => {
        const headerKeys = Object.keys(server.headers || {}).filter(Boolean);
        const toolCount = Array.isArray(server.toolCache) ? server.toolCache.length : 0;
        return `
          <div class="tcp-list-item grid gap-2">
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div>
                <div class="font-semibold">${escapeHtml(server.name)}</div>
                <div class="tcp-subtle">${escapeHtml(server.transport || "No transport set")}</div>
              </div>
              <div class="flex flex-wrap gap-2">
                <span class="${server.connected ? "tcp-badge-ok" : "tcp-badge-warn"}">${server.connected ? "Connected" : "Disconnected"}</span>
                <span class="${server.enabled ? "tcp-badge-info" : "tcp-badge-warn"}">${server.enabled ? "Enabled" : "Disabled"}</span>
                <span class="tcp-badge-info">Tools: ${toolCount}</span>
              </div>
            </div>
            ${
              server.lastError
                ? `<div class="rounded-xl border border-rose-700/60 bg-rose-950/20 px-2 py-1 text-xs text-rose-300">${escapeHtml(server.lastError)}</div>`
                : ""
            }
            ${
              headerKeys.length
                ? `<div class="tcp-subtle text-xs">Auth headers: ${escapeHtml(headerKeys.join(", "))}</div>`
                : `<div class="tcp-subtle text-xs">No stored auth headers.</div>`
            }
            <div class="flex flex-wrap gap-2">
              <button class="tcp-btn" data-action="edit" data-name="${encodeURIComponent(server.name)}">Edit</button>
              <button class="tcp-btn" data-action="${server.connected ? "disconnect" : "connect"}" data-name="${encodeURIComponent(server.name)}">
                ${server.connected ? "Disconnect" : "Connect"}
              </button>
              <button class="tcp-btn" data-action="refresh" data-name="${encodeURIComponent(server.name)}">Refresh</button>
              <button class="tcp-btn" data-action="toggle-enabled" data-name="${encodeURIComponent(server.name)}" data-enabled="${server.enabled ? "1" : "0"}">
                ${server.enabled ? "Disable" : "Enable"}
              </button>
              <button class="tcp-btn-danger" data-action="delete" data-name="${encodeURIComponent(server.name)}">Delete</button>
            </div>
          </div>
        `;
      })
      .join("") || '<p class="tcp-subtle">No MCP servers configured.</p>';

  listEl.querySelectorAll("button[data-action]").forEach((button) => {
    button.addEventListener("click", async () => {
      const action = String(button.dataset.action || "");
      const encoded = String(button.dataset.name || "");
      const name = encoded ? decodeURIComponent(encoded) : "";
      if (!name) return;
      try {
        if (action === "edit") {
          const row = servers.find((entry) => entry.name === name);
          prefillServerForEditing(row);
          return;
        } else if (action === "connect") {
          await state.client.mcp.connect(name);
          toast("ok", `Connected ${name}.`);
        } else if (action === "disconnect") {
          await state.client.mcp.disconnect(name);
          toast("ok", `Disconnected ${name}.`);
        } else if (action === "refresh") {
          await state.client.mcp.refresh(name);
          toast("ok", `Refreshed ${name}.`);
        } else if (action === "toggle-enabled") {
          const enabled = String(button.dataset.enabled || "0") === "1";
          await state.client.mcp.setEnabled(name, !enabled);
          toast("ok", `${!enabled ? "Enabled" : "Disabled"} ${name}.`);
        } else if (action === "delete") {
          await api(`/api/engine/mcp/${encodeURIComponent(name)}`, { method: "DELETE" });
          toast("ok", `Deleted ${name}.`);
        }
        await renderMcp(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });
  });
}
