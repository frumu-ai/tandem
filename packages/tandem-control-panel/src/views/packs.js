import { confirmActionModal } from "../app/dom.js";

export async function renderPacks(ctx) {
  const { byId, state, toast, renderIcons, escapeHtml, api, setRoute } = ctx;
  const embeddedInSettings = ctx?.embeddedInSettings === true;
  const trustBadgeClass = (badge) => {
    const value = String(badge || "").toLowerCase();
    if (value === "official") return "tcp-badge-info";
    if (value === "verified") return "tcp-badge-warn";
    return "tcp-badge-err";
  };
  const asArray = (value) => (Array.isArray(value) ? value : []);
  const packsApi = {
    list: () =>
      state.client?.packs?.list
        ? state.client.packs.list()
        : api("/api/engine/packs", { method: "GET" }),
    inspect: (selector) =>
      state.client?.packs?.inspect
        ? state.client.packs.inspect(selector)
        : api(`/api/engine/packs/${encodeURIComponent(selector)}`, { method: "GET" }),
    install: (request) =>
      state.client?.packs?.install
        ? state.client.packs.install(request)
        : api("/api/engine/packs/install", { method: "POST", body: JSON.stringify(request) }),
    uninstall: (request) =>
      state.client?.packs?.uninstall
        ? state.client.packs.uninstall(request)
        : api("/api/engine/packs/uninstall", { method: "POST", body: JSON.stringify(request) }),
    export: (request) =>
      state.client?.packs?.export
        ? state.client.packs.export(request)
        : api("/api/engine/packs/export", { method: "POST", body: JSON.stringify(request) }),
    updates: (selector) =>
      state.client?.packs?.updates
        ? state.client.packs.updates(selector)
        : api(`/api/engine/packs/${encodeURIComponent(selector)}/updates`, { method: "GET" }),
    update: (selector, request) =>
      state.client?.packs?.update
        ? state.client.packs.update(selector, request)
        : api(`/api/engine/packs/${encodeURIComponent(selector)}/update`, {
            method: "POST",
            body: JSON.stringify(request || {}),
          }),
  };
  const capabilitiesApi = {
    discovery: () =>
      state.client?.capabilities?.discovery
        ? state.client.capabilities.discovery()
        : api("/api/engine/capabilities/discovery", { method: "GET" }),
    readiness: (request) =>
      state.client?.capabilities?.readiness
        ? state.client.capabilities.readiness(request)
        : api("/api/engine/capabilities/readiness", {
            method: "POST",
            body: JSON.stringify(request || {}),
          }),
  };
  const parseCsv = (value) =>
    String(value || "")
      .split(",")
      .map((part) => part.trim())
      .filter((part) => part.length > 0);
  const unique = (items) => [...new Set(asArray(items).map((item) => String(item || "").trim()).filter(Boolean))];

  const app = byId("view");
  const movedCard = embeddedInSettings
    ? ""
    : `
    <div class="tcp-card mb-4">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 class="tcp-title">Moved To Settings</h3>
          <p class="tcp-subtle">Pack management now lives under Settings.</p>
        </div>
        <button id="packs-open-settings" class="tcp-btn"><i data-lucide="settings"></i> Open Settings</button>
      </div>
    </div>`;
  app.innerHTML = `
    ${movedCard}
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
      <div id="packs-inspect-summary" class="mt-3 hidden rounded-xl border border-slate-800 bg-slate-950/70 p-3 text-xs text-slate-200"></div>
      <pre id="packs-meta" class="mt-3 rounded-xl border border-slate-800 bg-slate-950/70 p-3 text-xs text-slate-300 hidden"></pre>
    </div>

    <div id="packs-list" class="grid gap-3"></div>

    <div class="tcp-card mt-4">
      <div class="mb-2 flex items-center justify-between gap-2">
        <h3 class="tcp-title">Skill Module Library</h3>
        <button id="presets-refresh-btn" class="tcp-btn"><i data-lucide="refresh-cw"></i> Refresh</button>
      </div>
      <div class="grid gap-2 md:grid-cols-3">
        <input id="presets-filter-text" class="tcp-input" placeholder="Filter by id/tag/layer" />
        <input id="presets-filter-publisher" class="tcp-input" placeholder="Publisher filter" />
        <input id="presets-filter-capability" class="tcp-input" placeholder="Required capability filter" />
      </div>
      <div id="presets-skill-list" class="mt-3 grid gap-2"></div>
    </div>

    <div class="tcp-card mt-4">
      <h3 class="tcp-title mb-2">Agent Preset Builder</h3>
      <p class="tcp-subtle mb-3">Create from a preset or compose from modules, preview prompt deterministically, and save/fork override.</p>
      <div class="grid gap-2 md:grid-cols-3">
        <select id="agent-builder-source" class="tcp-input"></select>
        <input id="agent-builder-fork-id" class="tcp-input" placeholder="Fork target id (optional)" />
        <button id="agent-builder-fork-btn" class="tcp-btn"><i data-lucide="copy-plus"></i> Fork Selected Preset</button>
      </div>
      <div class="mt-2 grid gap-2 md:grid-cols-2">
        <div>
          <label class="tcp-subtle block text-xs mb-1">Skill modules (multi-select)</label>
          <select id="agent-builder-modules" class="tcp-input h-36" multiple></select>
        </div>
        <div>
          <label class="tcp-subtle block text-xs mb-1">Base prompt</label>
          <textarea id="agent-builder-base" class="tcp-input h-36" placeholder="You are a pragmatic coding agent..."></textarea>
        </div>
      </div>
      <div class="mt-2 grid gap-2 md:grid-cols-2">
        <input id="agent-builder-required" class="tcp-input" placeholder="Additional required capabilities (csv)" />
        <input id="agent-builder-optional" class="tcp-input" placeholder="Additional optional capabilities (csv)" />
      </div>
      <div class="mt-2">
        <label class="tcp-subtle block text-xs mb-1">Extra fragments JSON (optional array of {id,phase,content})</label>
        <textarea id="agent-builder-fragments" class="tcp-input h-28" placeholder='[{"id":"tone.ops","phase":"style","content":"Be concise and operational."}]'></textarea>
      </div>
      <div class="mt-3 flex flex-wrap gap-2">
        <button id="agent-builder-preview-btn" class="tcp-btn"><i data-lucide="sparkles"></i> Compose Preview</button>
        <button id="agent-builder-summary-btn" class="tcp-btn"><i data-lucide="shield-check"></i> Capability Summary</button>
        <button id="agent-builder-save-btn" class="tcp-btn-primary"><i data-lucide="save"></i> Save Override</button>
      </div>
      <pre id="agent-builder-preview" class="mt-3 hidden rounded-xl border border-slate-800 bg-slate-950/70 p-3 text-xs text-slate-200"></pre>
      <pre id="agent-builder-summary" class="mt-2 hidden rounded-xl border border-slate-800 bg-slate-950/70 p-3 text-xs text-slate-200"></pre>
    </div>

    <div class="tcp-card mt-4">
      <h3 class="tcp-title mb-2">Automation Preset Builder</h3>
      <p class="tcp-subtle mb-3">Start from automation preset, bind steps to agents, and compute merged automation capabilities.</p>
      <div class="grid gap-2 md:grid-cols-3">
        <select id="automation-builder-source" class="tcp-input"></select>
        <input id="automation-builder-id" class="tcp-input" placeholder="Override id (required to save)" />
        <button id="automation-builder-add-task" class="tcp-btn"><i data-lucide="plus"></i> Add Step</button>
      </div>
      <div id="automation-builder-tasks" class="mt-3 grid gap-2"></div>
      <div class="mt-3 flex flex-wrap gap-2">
        <button id="automation-builder-summary-btn" class="tcp-btn"><i data-lucide="shield-check"></i> Capability Summary</button>
        <button id="automation-builder-save-btn" class="tcp-btn-primary"><i data-lucide="save"></i> Save Override</button>
      </div>
      <pre id="automation-builder-summary" class="mt-3 hidden rounded-xl border border-slate-800 bg-slate-950/70 p-3 text-xs text-slate-200"></pre>
    </div>
  `;

  const packsListEl = byId("packs-list");
  const metaEl = byId("packs-meta");
  const summaryEl = byId("packs-inspect-summary");
  const skillsHostEl = byId("presets-skill-list");
  const agentPreviewEl = byId("agent-builder-preview");
  const agentSummaryEl = byId("agent-builder-summary");
  const automationSummaryEl = byId("automation-builder-summary");
  const automationTasksEl = byId("automation-builder-tasks");

  let presetIndex = null;
  let automationTaskRows = [];

  const getSkills = () => asArray(presetIndex?.skill_modules);
  const getAgentPresets = () => asArray(presetIndex?.agent_presets);
  const getAutomationPresets = () => asArray(presetIndex?.automation_presets);
  const getPackPresets = () => asArray(presetIndex?.pack_presets);
  const findAgentPreset = (id) => getAgentPresets().find((row) => String(row?.id || "") === String(id || ""));
  const findAutomationPreset = (id) =>
    getAutomationPresets().find((row) => String(row?.id || "") === String(id || ""));

  const setPre = (el, value) => {
    if (!el) return;
    if (!value) {
      el.classList.add("hidden");
      el.textContent = "";
      return;
    }
    el.classList.remove("hidden");
    el.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  };

  const assertReadiness = async (workflowId, required, optional = []) => {
    const payload = await capabilitiesApi.readiness({
      workflow_id: workflowId,
      required_capabilities: unique(required),
      optional_capabilities: unique(optional),
    });
    const readiness = payload?.readiness || payload || {};
    if (readiness?.runnable) return readiness;
    const issues = asArray(readiness?.blocking_issues)
      .map((row) => String(row?.message || row?.code || "").trim())
      .filter(Boolean);
    const message = issues.length
      ? issues.join(" | ")
      : "Capability readiness failed. Connect/refresh MCP servers and verify bindings.";
    throw new Error(message);
  };

  const selectedModuleIds = () => {
    const select = byId("agent-builder-modules");
    return asArray(select?.selectedOptions).map((option) => String(option?.value || "")).filter(Boolean);
  };

  const selectedModulesRequiredCaps = () =>
    unique(
      selectedModuleIds().flatMap((id) => {
        const row = getSkills().find((item) => String(item?.id || "") === id);
        return asArray(row?.required_capabilities);
      })
    );

  const computeAgentCapabilityInput = () => {
    const required = unique([
      ...selectedModulesRequiredCaps(),
      ...parseCsv(byId("agent-builder-required")?.value),
    ]);
    const optionalRaw = unique(parseCsv(byId("agent-builder-optional")?.value));
    const optional = optionalRaw.filter((cap) => !required.includes(cap));
    return { required, optional };
  };

  const parseExtraFragments = () => {
    const raw = String(byId("agent-builder-fragments")?.value || "").trim();
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      throw new Error("Extra fragments JSON must be an array.");
    }
    return parsed
      .map((row) => ({
        id: String(row?.id || "").trim(),
        phase: String(row?.phase || "").trim() || "domain",
        content: String(row?.content || "").trim(),
      }))
      .filter((row) => row.id && row.content);
  };

  const collectAgentFragments = () => {
    const moduleFragments = selectedModuleIds().map((id) => ({
      id: `skill.${id}`,
      phase: "domain",
      content: `Enable skill module ${id}.`,
    }));
    return [...moduleFragments, ...parseExtraFragments()];
  };

  const renderAutomationRows = () => {
    const agentOptions = getAgentPresets()
      .map(
        (row) =>
          `<option value="${escapeHtml(String(row?.id || ""))}">${escapeHtml(String(row?.id || ""))}</option>`
      )
      .join("");
    if (!automationTaskRows.length) {
      automationTasksEl.innerHTML = '<p class="tcp-subtle">No steps yet. Click "Add Step" to add task-agent bindings.</p>';
      return;
    }
    automationTasksEl.innerHTML = automationTaskRows
      .map((row, index) => {
        const id = escapeHtml(row.id || `step_${index + 1}`);
        const required = escapeHtml((row.required || []).join(", "));
        const optional = escapeHtml((row.optional || []).join(", "));
        const selectedAgent = escapeHtml(String(row.agent_id || ""));
        return `
          <div class="rounded-xl border border-slate-800 bg-slate-950/60 p-3" data-task-row="${index}">
            <div class="grid gap-2 md:grid-cols-5">
              <input class="tcp-input" data-field="id" value="${id}" placeholder="step id" />
              <select class="tcp-input" data-field="agent_id">
                <option value="">(none)</option>
                ${agentOptions}
              </select>
              <input class="tcp-input" data-field="required" value="${required}" placeholder="required caps csv" />
              <input class="tcp-input" data-field="optional" value="${optional}" placeholder="optional caps csv" />
              <button class="tcp-btn" data-remove-row="${index}"><i data-lucide="trash-2"></i> Remove</button>
            </div>
          </div>
        `;
      })
      .join("");

    automationTasksEl.querySelectorAll("[data-task-row]").forEach((rowEl) => {
      const index = Number(rowEl.getAttribute("data-task-row"));
      const select = rowEl.querySelector('select[data-field="agent_id"]');
      if (select && automationTaskRows[index]) {
        select.value = String(automationTaskRows[index].agent_id || "");
      }
      rowEl.querySelectorAll("input,select").forEach((input) => {
        input.addEventListener("input", () => {
          const field = String(input.getAttribute("data-field") || "").trim();
          if (!field || !automationTaskRows[index]) return;
          const value = String(input.value || "");
          if (field === "required" || field === "optional") {
            automationTaskRows[index][field] = parseCsv(value);
          } else {
            automationTaskRows[index][field] = value.trim();
          }
        });
      });
    });

    automationTasksEl.querySelectorAll("[data-remove-row]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const index = Number(btn.getAttribute("data-remove-row"));
        automationTaskRows = automationTaskRows.filter((_, i) => i !== index);
        renderAutomationRows();
        renderIcons();
      });
    });

    renderIcons();
  };

  const renderPresetSelects = () => {
    const agentSource = byId("agent-builder-source");
    const modulesSelect = byId("agent-builder-modules");
    const automationSource = byId("automation-builder-source");

    const agentOptions = getAgentPresets();
    const autoOptions = getAutomationPresets();
    const skillOptions = getSkills();

    agentSource.innerHTML = [
      '<option value="">Select source agent preset</option>',
      ...agentOptions.map(
        (row) =>
          `<option value="${escapeHtml(String(row?.id || ""))}">${escapeHtml(String(row?.id || ""))} (${escapeHtml(
            String(row?.layer || "unknown")
          )})</option>`
      ),
    ].join("");

    modulesSelect.innerHTML = skillOptions
      .map(
        (row) =>
          `<option value="${escapeHtml(String(row?.id || ""))}">${escapeHtml(String(row?.id || ""))} (${escapeHtml(
            String(row?.layer || "unknown")
          )})</option>`
      )
      .join("");

    automationSource.innerHTML = [
      '<option value="">Select source automation preset</option>',
      ...autoOptions.map(
        (row) =>
          `<option value="${escapeHtml(String(row?.id || ""))}">${escapeHtml(String(row?.id || ""))} (${escapeHtml(
            String(row?.layer || "unknown")
          )})</option>`
      ),
    ].join("");
  };

  const renderSkills = () => {
    const skills = getSkills();
    const qText = String(byId("presets-filter-text")?.value || "").trim().toLowerCase();
    const qPub = String(byId("presets-filter-publisher")?.value || "")
      .trim()
      .toLowerCase();
    const qCap = String(byId("presets-filter-capability")?.value || "")
      .trim()
      .toLowerCase();
    const filtered = skills.filter((row) => {
      const id = String(row?.id || "").toLowerCase();
      const layer = String(row?.layer || "").toLowerCase();
      const tags = Array.isArray(row?.tags)
        ? row.tags.map((t) => String(t || "").toLowerCase())
        : [];
      const publisher = String(row?.publisher || "").toLowerCase();
      const caps = Array.isArray(row?.required_capabilities)
        ? row.required_capabilities.map((c) => String(c || "").toLowerCase())
        : [];
      const hayText = [id, layer, ...tags].join(" ");
      if (qText && !hayText.includes(qText)) return false;
      if (qPub && !publisher.includes(qPub)) return false;
      if (qCap && !caps.some((c) => c.includes(qCap))) return false;
      return true;
    });
    if (!filtered.length) {
      skillsHostEl.innerHTML = '<p class="tcp-subtle">No skill modules found.</p>';
      return;
    }
    skillsHostEl.innerHTML = filtered
      .map((row) => {
        const caps = Array.isArray(row?.required_capabilities) ? row.required_capabilities : [];
        const tags = Array.isArray(row?.tags) ? row.tags : [];
        return `
          <article class="rounded-xl border border-slate-800 bg-slate-950/60 p-3">
            <div class="flex items-center justify-between gap-2">
              <strong>${escapeHtml(String(row?.id || "unknown"))}</strong>
              <span class="tcp-subtle text-xs">${escapeHtml(String(row?.layer || "unknown"))}</span>
            </div>
            <div class="mt-1 text-xs text-slate-400">publisher: ${escapeHtml(String(row?.publisher || "unknown"))}</div>
            <div class="mt-1 text-xs text-slate-400">required capabilities: ${escapeHtml(String(caps.length))}</div>
            <div class="mt-1 text-xs text-slate-500">${escapeHtml(tags.join(", ") || "no tags")}</div>
          </article>
        `;
      })
      .join("");
  };

  const loadPresetIndex = async () => {
    try {
      const payload = await api("/api/presets/index");
      presetIndex = payload?.index || {
        skill_modules: [],
        agent_presets: [],
        automation_presets: [],
        pack_presets: [],
      };
      if (!Array.isArray(presetIndex?.pack_presets)) {
        presetIndex.pack_presets = [];
      }
      console.debug("[packs] loaded pack presets", getPackPresets().length);
      renderPresetSelects();
      renderSkills();
      renderAutomationRows();
    } catch (e) {
      presetIndex = {
        skill_modules: [],
        agent_presets: [],
        automation_presets: [],
        pack_presets: [],
      };
      renderPresetSelects();
      renderSkills();
      renderAutomationRows();
      toast("err", `Preset index load failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const setInspectSummary = (inspection) => {
    if (!summaryEl) return;
    const pack = inspection?.pack || {};
    const installed = pack?.installed || {};
    const trust = pack?.trust || {};
    const risk = pack?.risk || {};
    const sheet = pack?.permission_sheet || {};
    const badge = String(trust?.verification_badge || "unverified");
    const signature = String(trust?.signature || "unsigned");
    const required = asArray(sheet?.required_capabilities);
    const optional = asArray(sheet?.optional_capabilities);
    const providerSpecific = asArray(sheet?.provider_specific_dependencies);
    const routines = asArray(sheet?.routines_declared);
    const packLabel = `${String(installed?.name || "unknown")}@${String(installed?.version || "unknown")}`;
    summaryEl.classList.remove("hidden");
    summaryEl.innerHTML = `
      <div class="flex items-center justify-between gap-2">
        <strong class="text-sm">${escapeHtml(packLabel)}</strong>
        <span class="${trustBadgeClass(badge)}">${escapeHtml(badge)}</span>
      </div>
      <div class="mt-2 grid gap-1 text-[11px] text-slate-300">
        <div>signature: <span class="font-mono">${escapeHtml(signature)}</span></div>
        <div>risk level: <span class="font-mono">${escapeHtml(String(sheet?.risk_level || "standard"))}</span></div>
        <div>required capabilities: <span class="font-mono">${required.length}</span></div>
        <div>optional capabilities: <span class="font-mono">${optional.length}</span></div>
        <div>provider-specific deps: <span class="font-mono">${providerSpecific.length}</span></div>
        <div>routines declared: <span class="font-mono">${routines.length}</span></div>
        <div>routines enabled: <span class="font-mono">${escapeHtml(String(risk?.routines_enabled ?? false))}</span></div>
      </div>
    `;
  };
  const setMeta = (value) => {
    if (!metaEl) return;
    if (!value) {
      metaEl.classList.add("hidden");
      metaEl.textContent = "";
      summaryEl?.classList.add("hidden");
      return;
    }
    metaEl.classList.remove("hidden");
    metaEl.textContent = JSON.stringify(value, null, 2);
  };

  const loadPacks = async () => {
    try {
      const payload = await packsApi.list();
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
            const inspected = await packsApi.inspect(id);
            setInspectSummary(inspected);
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
            const exported = await packsApi.export({ pack_id: id });
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
            const updates = await packsApi.updates(id);
            setMeta(updates);
            const count = Array.isArray(updates?.updates) ? updates.updates.length : 0;
            const needsReapproval = !!updates?.reapproval_required;
            toast(
              needsReapproval ? "warn" : "info",
              needsReapproval
                ? `Updates available: ${count}. Permission scope changed; re-approval required.`
                : `Updates available: ${count}`
            );
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
            const updated = await packsApi.update(id, {});
            setMeta(updated);
            const needsReapproval = !!updated?.reapproval_required;
            toast(
              needsReapproval ? "warn" : "info",
              updated?.reason ||
                (updated?.updated
                  ? "Pack updated"
                  : needsReapproval
                    ? "No update applied. Permission re-approval required."
                    : "No update applied")
            );
          } catch (e) {
            toast("err", `Update failed: ${e instanceof Error ? e.message : String(e)}`);
          }
        });
      });

      packsListEl.querySelectorAll("[data-pack-remove]").forEach((btn) => {
        btn.addEventListener("click", async () => {
          const id = btn.getAttribute("data-pack-remove");
          if (!id) return;
          const approved = await confirmActionModal({
            title: "Uninstall pack",
            message: `Remove installed pack ${id}? This does not delete exported zip files.`,
            confirmLabel: "Uninstall",
          });
          if (!approved) return;
          try {
            await packsApi.uninstall({ pack_id: id });
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
  if (!embeddedInSettings) {
    byId("packs-open-settings")?.addEventListener("click", () => setRoute("settings"));
  }
  byId("presets-refresh-btn")?.addEventListener("click", () => void loadPresetIndex());
  byId("presets-filter-text")?.addEventListener("input", renderSkills);
  byId("presets-filter-publisher")?.addEventListener("input", renderSkills);
  byId("presets-filter-capability")?.addEventListener("input", renderSkills);

  byId("packs-cap-discovery-btn")?.addEventListener("click", async () => {
    try {
      const discovery = await capabilitiesApi.discovery();
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
      const payload = await packsApi.install({
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

  byId("agent-builder-fork-btn")?.addEventListener("click", async () => {
    const sourceId = String(byId("agent-builder-source")?.value || "").trim();
    if (!sourceId) {
      toast("err", "Select a source agent preset first.");
      return;
    }
    const source = findAgentPreset(sourceId);
    if (!source?.path) {
      toast("err", "Selected source preset has no readable path.");
      return;
    }
    try {
      const payload = await api("/api/presets/fork", {
        method: "POST",
        body: JSON.stringify({
          kind: "agent_preset",
          source_path: source.path,
          target_id: String(byId("agent-builder-fork-id")?.value || "").trim() || undefined,
        }),
      });
      setMeta(payload);
      toast("ok", `Forked to ${payload?.path || "override"}`);
      await loadPresetIndex();
    } catch (e) {
      toast("err", `Fork failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });

  byId("agent-builder-preview-btn")?.addEventListener("click", async () => {
    try {
      const payload = await api("/api/presets/compose/preview", {
        method: "POST",
        body: JSON.stringify({
          base_prompt: String(byId("agent-builder-base")?.value || ""),
          fragments: collectAgentFragments(),
        }),
      });
      const composition = payload?.composition || {};
      setPre(
        agentPreviewEl,
        {
          composition_hash: composition?.composition_hash,
          ordered_fragment_ids: composition?.ordered_fragment_ids,
          prompt: composition?.prompt,
        }
      );
    } catch (e) {
      toast("err", `Compose preview failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });

  byId("agent-builder-summary-btn")?.addEventListener("click", async () => {
    try {
      const payload = await api("/api/presets/capability_summary", {
        method: "POST",
        body: JSON.stringify({
          agent: computeAgentCapabilityInput(),
          tasks: [],
        }),
      });
      setPre(agentSummaryEl, payload?.summary || payload);
    } catch (e) {
      toast("err", `Capability summary failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });

  byId("agent-builder-save-btn")?.addEventListener("click", async () => {
    const sourceId = String(byId("agent-builder-source")?.value || "").trim();
    const targetId = String(byId("agent-builder-fork-id")?.value || "").trim() || sourceId;
    if (!targetId) {
      toast("err", "Provide target id (or select source preset)." );
      return;
    }
    const caps = computeAgentCapabilityInput();
    try {
      await assertReadiness(`agent:${targetId}`, caps.required, caps.optional);
    } catch (e) {
      toast("err", `Readiness blocked save: ${e instanceof Error ? e.message : String(e)}`);
      return;
    }
    const moduleIds = selectedModuleIds();
    const basePrompt = String(byId("agent-builder-base")?.value || "").trim();
    const yaml = [
      `id: ${targetId}`,
      "version: 0.1.0",
      "publisher: local.user",
      "description: Project override agent preset",
      "tags:",
      "  - override",
      "modules:",
      ...moduleIds.map((id) => `  - ${id}`),
      "capabilities:",
      "  required:",
      ...(caps.required.length ? caps.required.map((cap) => `    - ${cap}`) : ["    - "]),
      "  optional:",
      ...(caps.optional.length ? caps.optional.map((cap) => `    - ${cap}`) : ["    - "]),
      "prompt:",
      "  base: |",
      ...(basePrompt ? basePrompt.split("\n").map((line) => `    ${line}`) : ["    "]),
      "  fragments:",
      ...collectAgentFragments().map((fragment) => `    - id: ${fragment.id}\n      phase: ${fragment.phase}\n      content: ${JSON.stringify(fragment.content)}`),
      "",
    ].join("\n");

    try {
      const payload = await api(`/api/presets/overrides/agent_preset/${encodeURIComponent(targetId)}`, {
        method: "PUT",
        body: JSON.stringify({ content: yaml }),
      });
      setMeta(payload);
      toast("ok", `Saved agent override ${targetId}`);
      await loadPresetIndex();
    } catch (e) {
      toast("err", `Save override failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });

  byId("automation-builder-add-task")?.addEventListener("click", () => {
    automationTaskRows.push({
      id: `step_${automationTaskRows.length + 1}`,
      agent_id: "",
      required: [],
      optional: [],
    });
    renderAutomationRows();
  });

  byId("automation-builder-source")?.addEventListener("change", () => {
    const sourceId = String(byId("automation-builder-source")?.value || "").trim();
    const source = findAutomationPreset(sourceId);
    if (!sourceId || !source) {
      automationTaskRows = [];
      renderAutomationRows();
      return;
    }
    automationTaskRows = [
      {
        id: `${sourceId}_step_1`,
        agent_id: String(byId("agent-builder-source")?.value || "").trim(),
        required: unique(asArray(source.required_capabilities)),
        optional: [],
      },
    ];
    renderAutomationRows();
  });

  byId("automation-builder-summary-btn")?.addEventListener("click", async () => {
    try {
      const selectedAgentId = String(byId("agent-builder-source")?.value || "").trim();
      const selectedAgent = findAgentPreset(selectedAgentId);
      const agent = {
        required: unique(asArray(selectedAgent?.required_capabilities).concat(computeAgentCapabilityInput().required)),
        optional: computeAgentCapabilityInput().optional,
      };
      const tasks = automationTaskRows.map((row) => {
        const rowAgent = findAgentPreset(row.agent_id);
        const rowAgentCaps = unique(asArray(rowAgent?.required_capabilities));
        const required = unique(rowAgentCaps.concat(asArray(row.required)));
        const optional = unique(asArray(row.optional)).filter((cap) => !required.includes(cap));
        return { required, optional };
      });
      const payload = await api("/api/presets/capability_summary", {
        method: "POST",
        body: JSON.stringify({ agent, tasks }),
      });
      setPre(automationSummaryEl, payload?.summary || payload);
    } catch (e) {
      toast("err", `Automation summary failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });

  byId("automation-builder-save-btn")?.addEventListener("click", async () => {
    const id = String(byId("automation-builder-id")?.value || "").trim();
    if (!id) {
      toast("err", "Automation override id is required.");
      return;
    }
    let summary = null;
    let automationRequired = [];
    let automationOptional = [];
    try {
      const selectedAgentId = String(byId("agent-builder-source")?.value || "").trim();
      const selectedAgent = findAgentPreset(selectedAgentId);
      const agent = {
        required: unique(asArray(selectedAgent?.required_capabilities).concat(computeAgentCapabilityInput().required)),
        optional: computeAgentCapabilityInput().optional,
      };
      const tasks = automationTaskRows.map((row) => {
        const rowAgent = findAgentPreset(row.agent_id);
        const rowAgentCaps = unique(asArray(rowAgent?.required_capabilities));
        const required = unique(rowAgentCaps.concat(asArray(row.required)));
        const optional = unique(asArray(row.optional)).filter((cap) => !required.includes(cap));
        return { required, optional };
      });
      const summaryResp = await api("/api/presets/capability_summary", {
        method: "POST",
        body: JSON.stringify({ agent, tasks }),
      });
      summary = summaryResp?.summary || null;
      automationRequired = unique(asArray(summary?.automation?.required));
      automationOptional = unique(asArray(summary?.automation?.optional));
    } catch {
      summary = null;
    }
    try {
      await assertReadiness(`automation:${id}`, automationRequired, automationOptional);
    } catch (e) {
      toast("err", `Readiness blocked save: ${e instanceof Error ? e.message : String(e)}`);
      return;
    }

    const yaml = [
      `id: ${id}`,
      "version: 0.1.0",
      "publisher: local.user",
      "description: Project override automation preset",
      "tags:",
      "  - override",
      "tasks:",
      ...(automationTaskRows.length
        ? automationTaskRows.flatMap((row) => [
            `  - id: ${row.id || "step"}`,
            `    agent_preset: ${row.agent_id || ""}`,
            "    capabilities:",
            "      required:",
            ...(asArray(row.required).length ? asArray(row.required).map((cap) => `        - ${cap}`) : ["        - "]),
            "      optional:",
            ...(asArray(row.optional).length ? asArray(row.optional).map((cap) => `        - ${cap}`) : ["        - "]),
          ])
        : ["  - id: step_1", "    agent_preset: "]),
      "capabilities:",
      "  required:",
      ...((summary?.automation?.required || []).length
        ? summary.automation.required.map((cap) => `    - ${cap}`)
        : ["    - "]),
      "  optional:",
      ...((summary?.automation?.optional || []).length
        ? summary.automation.optional.map((cap) => `    - ${cap}`)
        : ["    - "]),
      "",
    ].join("\n");

    try {
      const payload = await api(`/api/presets/overrides/automation_preset/${encodeURIComponent(id)}`, {
        method: "PUT",
        body: JSON.stringify({ content: yaml }),
      });
      setMeta(payload);
      toast("ok", `Saved automation override ${id}`);
      await loadPresetIndex();
    } catch (e) {
      toast("err", `Save automation override failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  });

  await loadPacks();
  await loadPresetIndex();
}
