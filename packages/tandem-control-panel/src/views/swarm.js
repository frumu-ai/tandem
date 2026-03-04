import { subscribeSse } from "../services/sse.js";

function pickStatusClass(status) {
  const normalized = String(status || "").toLowerCase();
  if (normalized.includes("fail") || normalized.includes("error") || normalized.includes("cancel")) return "tcp-badge-err";
  if (
    normalized.includes("wait") ||
    normalized.includes("queue") ||
    normalized.includes("new") ||
    normalized.includes("block") ||
    normalized.includes("paused")
  ) {
    return "tcp-badge-warn";
  }
  return "tcp-badge-ok";
}

function normalizeMcpServers(raw) {
  if (Array.isArray(raw)) {
    return raw
      .map((entry) => {
        if (!entry || typeof entry !== "object") return null;
        const name = String(entry.name || "").trim();
        if (!name) return null;
        return {
          name,
          connected: !!entry.connected,
          enabled: entry.enabled !== false,
        };
      })
      .filter(Boolean)
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  if (!raw || typeof raw !== "object") return [];
  if (Array.isArray(raw.servers)) return normalizeMcpServers(raw.servers);

  return Object.entries(raw)
    .map(([name, row]) => ({
      name: String(name || "").trim(),
      connected: !!row?.connected,
      enabled: row?.enabled !== false,
    }))
    .filter((row) => row.name)
    .sort((a, b) => a.name.localeCompare(b.name));
}

function swarmFormHasFocus() {
  const active = document.activeElement;
  if (!active || !(active instanceof HTMLElement)) return false;
  if (!active.closest("[data-swarm-form]")) return false;
  const tag = String(active.tagName || "").toLowerCase();
  if (tag === "textarea" || tag === "select") return true;
  if (tag !== "input") return false;
  const type = String(active.getAttribute("type") || "text").toLowerCase();
  return !["button", "submit", "reset", "checkbox", "radio", "range"].includes(type);
}

function swarmRefreshLocked(state) {
  return Number(state?.__swarmUiLockUntil || 0) > Date.now();
}

function setSwarmRefreshLock(state, msFromNow) {
  const until = Date.now() + Math.max(0, Number(msFromNow || 0));
  state.__swarmUiLockUntil = Math.max(Number(state.__swarmUiLockUntil || 0), until);
}

function ageText(ts) {
  const ms = Number(ts || 0);
  if (!ms) return "unknown";
  const delta = Math.max(0, Date.now() - ms);
  if (delta < 1000) return "just now";
  const sec = Math.floor(delta / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  return `${day}d ago`;
}

function runStages() {
  return [
    "queued",
    "planning",
    "awaiting_approval",
    "running",
    "paused",
    "blocked",
    "completed",
    "failed",
    "cancelled",
  ];
}

function stepColumns() {
  return [
    { key: "pending", label: "Pending" },
    { key: "runnable", label: "Runnable" },
    { key: "in_progress", label: "In Progress" },
    { key: "blocked", label: "Blocked" },
    { key: "done", label: "Done" },
    { key: "failed", label: "Failed" },
  ];
}

function normalizeStepStatus(raw) {
  const status = String(raw || "pending").trim().toLowerCase();
  if (["pending", "runnable", "in_progress", "blocked", "done", "failed"].includes(status)) {
    return status;
  }
  if (status.includes("run") || status.includes("active")) return "in_progress";
  if (status.includes("block") || status.includes("wait")) return "blocked";
  if (status.includes("done") || status.includes("complete")) return "done";
  if (status.includes("fail") || status.includes("error")) return "failed";
  return "pending";
}

function copyText(text) {
  const value = String(text || "");
  if (!value) return Promise.resolve();
  if (navigator?.clipboard?.writeText) return navigator.clipboard.writeText(value);
  const area = document.createElement("textarea");
  area.value = value;
  area.style.position = "fixed";
  area.style.left = "-10000px";
  document.body.appendChild(area);
  area.select();
  document.execCommand("copy");
  area.remove();
  return Promise.resolve();
}

function buildStepCard(task, runId, escapeHtml) {
  const stepId = String(task.taskId || "task");
  const title = String(task.title || task.taskId || "Untitled task");
  const stepStatus = String(task.stepStatus || "pending");
  const reason = String(task.statusReason || "");
  const updated = Number(task.lastUpdateMs || 0);
  const sessionId = String(task.sessionId || "");
  const copyPayload = [`runId=${runId}`, `stepId=${stepId}`, sessionId ? `sessionId=${sessionId}` : ""]
    .filter(Boolean)
    .join("\n");

  return `<article class="tcp-list-item border border-slate-700/70 bg-slate-950/35">
    <div class="mb-2 flex items-start justify-between gap-2">
      <div class="min-w-0">
        <div class="truncate font-semibold text-slate-100" title="${escapeHtml(title)}">${escapeHtml(title)}</div>
        <div class="truncate text-xs text-slate-400">${escapeHtml(stepId)}</div>
      </div>
      <span class="${pickStatusClass(stepStatus)}">${escapeHtml(stepStatus)}</span>
    </div>
    <div class="grid gap-1 text-xs text-slate-300">
      <div><span class="text-slate-400">Reason:</span> ${escapeHtml(reason || "-")}</div>
      <div><span class="text-slate-400">Updated:</span> ${escapeHtml(ageText(updated))}</div>
      <div><span class="text-slate-400">Run:</span> ${escapeHtml(runId)}</div>
      <div><span class="text-slate-400">Session:</span> ${escapeHtml(sessionId || "-")}</div>
    </div>
    <div class="mt-3 flex flex-wrap gap-2">
      <button class="tcp-btn h-7 px-2 text-xs" data-step-copy="${escapeHtml(copyPayload)}">Copy IDs</button>
      <button class="tcp-btn h-7 px-2 text-xs" data-step-retry="${escapeHtml(stepId)}">Retry</button>
      ${sessionId ? `<button class="tcp-btn h-7 px-2 text-xs" data-step-open-session="${escapeHtml(sessionId)}">Open Session</button>` : ""}
    </div>
  </article>`;
}

export async function renderSwarm(ctx, options = {}) {
  const { api, byId, escapeHtml, toast, state, addCleanup, setRoute } = ctx;
  if (state.route !== "swarm") return;
  const force = options?.force === true;
  if (!force && state.__swarmRenderedOnce && (swarmFormHasFocus() || swarmRefreshLocked(state))) return;
  if (state.__swarmRenderInFlight) return;
  state.__swarmRenderInFlight = true;
  try {
    const renderRouteSnapshot = state.route;
    if (state.__swarmLiveCleanup && Array.isArray(state.__swarmLiveCleanup)) {
      for (const fn of state.__swarmLiveCleanup) {
        try {
          fn();
        } catch {
          // ignore cleanup failure
        }
      }
    }
    state.__swarmLiveCleanup = [];

    const [status, providerCatalog, providerConfig, mcpRaw] = await Promise.all([
      api("/api/swarm/status").catch(() => ({ status: "error" })),
      state.client?.providers?.catalog?.().catch(() => ({ all: [] })),
      state.client?.providers?.config?.().catch(() => ({ default: "", providers: {} })),
      state.client?.mcp?.list?.().catch(() => ({})),
    ]);
    if (state.route !== renderRouteSnapshot) return;

    if (!state.__swarmDraft || typeof state.__swarmDraft !== "object") state.__swarmDraft = {};
    const draft = state.__swarmDraft;
    if (!draft.workspaceRoot) draft.workspaceRoot = String(status.workspaceRoot || "").trim();
    if (!draft.objective) draft.objective = String(status.objective || "Ship a small feature end-to-end").trim();
    if (!draft.maxTasks) draft.maxTasks = String(status.maxTasks || 3);
    if (!draft.modelProvider) draft.modelProvider = String(status.modelProvider || providerConfig?.default || "").trim();

    const providers = Array.isArray(providerCatalog?.all)
      ? providerCatalog.all
          .map((row) => ({
            id: String(row?.id || "").trim(),
            models: Object.keys(row?.models || {}).filter(Boolean),
          }))
          .filter((row) => row.id)
          .sort((a, b) => a.id.localeCompare(b.id))
      : [];
    const modelsForProvider = providers.find((row) => row.id === draft.modelProvider)?.models || [];
    if (!draft.modelId) {
      const configuredDefault = String(providerConfig?.providers?.[draft.modelProvider]?.default_model || "").trim();
      draft.modelId = String(status.modelId || configuredDefault || modelsForProvider[0] || "").trim();
    }
    if (!Array.isArray(draft.mcpServers)) {
      draft.mcpServers = Array.isArray(status.mcpServers)
        ? status.mcpServers.map((v) => String(v).trim()).filter(Boolean)
        : [];
    }

    const selectedWorkspace = String(draft.workspaceRoot || status.workspaceRoot || "").trim();
    const runsPayload = await api(`/api/swarm/runs?workspace=${encodeURIComponent(selectedWorkspace)}`).catch(() => ({ runs: [] }));
    const runs = Array.isArray(runsPayload?.runs) ? runsPayload.runs : [];
    if (!state.__swarmSelectedRunId) {
      state.__swarmSelectedRunId = String(status.currentRunId || runs[0]?.run_id || "").trim();
    }
    if (state.__swarmSelectedRunId && !runs.some((r) => String(r?.run_id || "") === state.__swarmSelectedRunId)) {
      state.__swarmSelectedRunId = String(runs[0]?.run_id || "").trim();
    }

    const selectedRunId = String(state.__swarmSelectedRunId || "").trim();
    const runPayload = selectedRunId
      ? await api(`/api/swarm/run/${encodeURIComponent(selectedRunId)}`).catch(() => ({ run: null, events: [], tasks: [] }))
      : { run: null, events: [], tasks: [] };
    const run = runPayload?.run || null;
    const events = Array.isArray(runPayload?.events) ? runPayload.events : [];
    const blackboard = runPayload?.blackboard || null;
    const tasks = Array.isArray(runPayload?.tasks) ? runPayload.tasks : [];
    const runStatus = String(run?.status || status.status || "idle").trim().toLowerCase();
    const needsApproval = runStatus === "awaiting_approval" || runStatus === "planning";
    const seededLocally = events.some((evt) => String(evt?.type || "").trim() === "plan_seeded_local");

    const grouped = {
      pending: [],
      runnable: [],
      in_progress: [],
      blocked: [],
      done: [],
      failed: [],
    };
    for (const task of tasks) {
      grouped[normalizeStepStatus(task.stepStatus || task.status)].push(task);
    }
    for (const key of Object.keys(grouped)) {
      grouped[key].sort((a, b) => (b.lastUpdateMs || 0) - (a.lastUpdateMs || 0));
    }

    const selectedMcp = new Set(draft.mcpServers.map((v) => String(v || "").toLowerCase()));
    const connectedMcp = normalizeMcpServers(mcpRaw).filter((row) => row.connected && row.enabled);

    const viewEl = byId("view");
    viewEl.innerHTML = `
      <div class="tcp-card" data-swarm-form="1">
        <div class="mb-3 flex items-center justify-between gap-3">
          <h3 class="tcp-title flex items-center gap-2"><i data-lucide="cpu"></i> Swarm Context Runs</h3>
          <span class="${pickStatusClass(run?.status || status.status || "idle")}">${escapeHtml(String(run?.status || status.status || "idle"))}</span>
        </div>
        <p class="mb-3 rounded-xl border border-slate-700/60 bg-slate-900/25 px-3 py-2 text-xs text-slate-300">
          Control Panel Swarm now uses canonical <code>context/runs</code> with explicit planning approval.
        </p>

        <div class="mb-3 grid gap-3 md:grid-cols-[1fr_160px_auto]">
          <input id="swarm-root" class="tcp-input" value="${escapeHtml(draft.workspaceRoot || "")}" placeholder="workspace root" />
          <input id="swarm-max" class="tcp-input" type="number" min="1" value="${escapeHtml(String(draft.maxTasks || 3))}" />
          <button id="swarm-start" class="tcp-btn-primary"><i data-lucide="play"></i> New Run</button>
        </div>

        <div class="grid gap-3 lg:grid-cols-2">
          <div class="grid gap-2">
            <label for="swarm-model-provider" class="text-xs uppercase tracking-wide text-slate-400">Model Provider</label>
            <select id="swarm-model-provider" class="tcp-select">
              <option value="">Default provider/model</option>
              ${providers
                .map((row) => `<option value="${escapeHtml(row.id)}" ${row.id === draft.modelProvider ? "selected" : ""}>${escapeHtml(row.id)}</option>`)
                .join("")}
            </select>
          </div>
          <div class="grid gap-2">
            <label for="swarm-model-id" class="text-xs uppercase tracking-wide text-slate-400">Model ID</label>
            <select id="swarm-model-id" class="tcp-select" ${draft.modelProvider ? "" : "disabled"}>
              ${
                draft.modelProvider
                  ? (providers.find((row) => row.id === draft.modelProvider)?.models || [])
                      .map(
                        (modelId) =>
                          `<option value="${escapeHtml(modelId)}" ${modelId === draft.modelId ? "selected" : ""}>${escapeHtml(modelId)}</option>`
                      )
                      .join("")
                  : '<option value="">Uses provider default</option>'
              }
            </select>
          </div>
        </div>

        <div class="mt-3 grid gap-2">
          <label for="swarm-objective" class="text-xs uppercase tracking-wide text-slate-400">Objective (Markdown)</label>
          <textarea id="swarm-objective" class="tcp-input min-h-[180px] resize-y leading-relaxed" placeholder="Describe the swarm objective in markdown...">${escapeHtml(draft.objective || "")}</textarea>
        </div>

        <div class="mt-3 grid gap-2">
          <div class="text-xs uppercase tracking-wide text-slate-400">MCP Servers</div>
          <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            ${
              connectedMcp.length
                ? connectedMcp
                    .map(
                      (row) => `<label class="tcp-list-item flex items-center gap-2 text-sm">
                    <input type="checkbox" data-swarm-mcp-option="${escapeHtml(row.name)}" ${selectedMcp.has(row.name.toLowerCase()) ? "checked" : ""} />
                    <span>${escapeHtml(row.name)}</span>
                  </label>`
                    )
                    .join("")
                : '<p class="tcp-subtle">No connected MCP servers found. Connect them in Settings > MCP.</p>'
            }
          </div>
        </div>
      </div>

      <div class="grid gap-4 xl:grid-cols-[minmax(260px,320px)_minmax(0,1fr)_minmax(320px,420px)]">
        <aside class="tcp-card">
          <div class="mb-3 flex items-center justify-between gap-2">
            <h3 class="tcp-title">Context Runs</h3>
            <span class="tcp-subtle text-xs">${runs.length} runs</span>
          </div>
          <div class="grid max-h-[620px] gap-2 overflow-auto">
            ${
              runs.length
                ? runs
                    .map((row) => {
                      const runId = String(row?.run_id || "");
                      const active = runId === selectedRunId;
                      return `<button class="tcp-list-item text-left ${active ? "border border-emerald-600/60" : ""}" data-run-select="${escapeHtml(runId)}">
                        <div class="flex items-center justify-between gap-2">
                          <span class="${pickStatusClass(row?.status)}">${escapeHtml(String(row?.status || "unknown"))}</span>
                          <span class="text-xs text-slate-500">${escapeHtml(ageText(row?.updated_at_ms))}</span>
                        </div>
                        <div class="mt-1 truncate text-xs text-slate-300">${escapeHtml(runId)}</div>
                        <div class="mt-1 line-clamp-2 text-xs text-slate-500">${escapeHtml(String(row?.objective || ""))}</div>
                      </button>`;
                    })
                    .join("")
                : '<p class="tcp-subtle">No runs found for this workspace.</p>'
            }
          </div>
        </aside>

        <div class="tcp-card">
          <div class="mb-3 flex items-center justify-between gap-3">
            <h3 class="tcp-title">Swarm Kanban</h3>
            <div class="flex flex-wrap gap-2">
              <button id="swarm-approve" class="tcp-btn h-8 px-3 text-xs" ${selectedRunId ? "" : "disabled"}>${needsApproval ? "Approve Plan & Start Execution" : "Approve Plan"}</button>
              <button id="swarm-pause" class="tcp-btn h-8 px-3 text-xs" ${selectedRunId ? "" : "disabled"}>Pause</button>
              <button id="swarm-resume" class="tcp-btn h-8 px-3 text-xs" ${selectedRunId ? "" : "disabled"}>Resume</button>
              <button id="swarm-cancel" class="tcp-btn-danger h-8 px-3 text-xs" ${selectedRunId ? "" : "disabled"}>Cancel</button>
            </div>
          </div>
          ${
            needsApproval
              ? `<div class="mb-3 rounded-xl border border-amber-700/60 bg-amber-950/25 px-3 py-2 text-sm text-amber-300">
                  <strong>Execution is waiting for approval.</strong> Steps are visible, but no LLM calls will run until you click <em>Approve Plan & Start Execution</em>.
                </div>`
              : ""
          }
          ${
            seededLocally
              ? `<div class="mb-3 rounded-xl border border-slate-700/60 bg-slate-900/25 px-3 py-2 text-xs text-slate-300">
                  Initial tasks were seeded locally from the objective text (non-LLM) so the board can render before execution starts.
                </div>`
              : ""
          }
          <div class="mb-3 rounded-xl border border-slate-700/60 bg-slate-900/20 px-3 py-2 text-xs text-slate-300">
            <div><strong>Run ID:</strong> ${escapeHtml(selectedRunId || "-")}</div>
            <div><strong>Workspace:</strong> ${escapeHtml(String(run?.workspace?.canonical_path || draft.workspaceRoot || "-"))}</div>
            <div><strong>Why next step:</strong> ${escapeHtml(String(run?.why_next_step || "-") || "-")}</div>
          </div>
          <div class="grid gap-3 xl:grid-cols-3 2xl:grid-cols-6">
            ${stepColumns()
              .map((col) => {
                const entries = grouped[col.key] || [];
                return `<section class="rounded-xl border border-slate-700/70 bg-slate-950/30 p-2">
                  <div class="mb-2 flex items-center justify-between gap-2">
                    <h4 class="text-xs font-semibold uppercase tracking-wide text-slate-300">${escapeHtml(col.label)}</h4>
                    <span class="tcp-badge-info">${entries.length}</span>
                  </div>
                  <div class="grid max-h-[520px] gap-2 overflow-auto">
                    ${entries.map((task) => buildStepCard(task, selectedRunId, escapeHtml)).join("") || '<p class="px-2 py-1 text-xs text-slate-500">No steps</p>'}
                  </div>
                </section>`;
              })
              .join("")}
          </div>
        </div>

        <aside class="grid gap-4">
          <div class="tcp-card">
            <h3 class="tcp-title mb-3">Timeline</h3>
            <div class="grid max-h-[300px] gap-2 overflow-auto">
              ${
                events.length
                  ? events
                      .slice()
                      .reverse()
                      .map(
                        (evt) => `<div class="tcp-list-item">
                        <div class="flex items-center justify-between gap-2">
                          <span class="text-xs text-slate-400">${new Date(Number(evt?.ts_ms || Date.now())).toLocaleTimeString()}</span>
                          <span class="${pickStatusClass(evt?.status)}">${escapeHtml(String(evt?.status || "unknown"))}</span>
                        </div>
                        <div class="mt-1 text-sm text-slate-200">${escapeHtml(String(evt?.type || "event"))}</div>
                        <div class="mt-1 text-xs text-slate-500">${escapeHtml(String(evt?.step_id || "run"))}</div>
                      </div>`
                      )
                      .join("")
                  : '<p class="tcp-subtle">No timeline events yet.</p>'
              }
            </div>
          </div>

          <div class="tcp-card">
            <h3 class="tcp-title mb-3">Blackboard</h3>
            <div class="text-xs text-slate-300">
              <div><strong>Facts:</strong> ${Number(blackboard?.facts?.length || 0)}</div>
              <div><strong>Decisions:</strong> ${Number(blackboard?.decisions?.length || 0)}</div>
              <div><strong>Open questions:</strong> ${Number(blackboard?.open_questions?.length || 0)}</div>
              <div class="mt-2 text-slate-400">${escapeHtml(String(blackboard?.summaries?.rolling || "No rolling summary yet."))}</div>
            </div>
          </div>

          <div class="tcp-card">
            <h3 class="tcp-title mb-3">Events Log</h3>
            <pre class="tcp-code max-h-[240px] overflow-auto">${escapeHtml(
              events
                .slice(-120)
                .map((evt) => `[${new Date(Number(evt?.ts_ms || Date.now())).toLocaleTimeString()}] ${evt?.type || "event"} ${evt?.status || ""}`.trim())
                .join("\n")
            )}</pre>
          </div>
        </aside>
      </div>
    `;

    const formRoot = viewEl.querySelector("[data-swarm-form]");
    formRoot?.addEventListener("pointerdown", () => setSwarmRefreshLock(state, 1500));
    formRoot?.addEventListener("focusin", () => setSwarmRefreshLock(state, 60_000));
    formRoot?.addEventListener("focusout", () => setSwarmRefreshLock(state, 1200));

    const setDraftValue = (key, value) => {
      draft[key] = value;
    };

    const collectCurrentFormState = () => {
      const workspaceRoot = String(byId("swarm-root")?.value ?? draft.workspaceRoot ?? "").trim();
      const objective = String(byId("swarm-objective")?.value ?? draft.objective ?? "").trim();
      const maxTasks = Number.parseInt(String(byId("swarm-max")?.value ?? draft.maxTasks ?? "3"), 10) || 3;
      const modelProvider = String(byId("swarm-model-provider")?.value ?? draft.modelProvider ?? "").trim();
      const modelId = String(byId("swarm-model-id")?.value ?? draft.modelId ?? "").trim();
      const mcpServers = [...viewEl.querySelectorAll("[data-swarm-mcp-option]:checked")]
        .map((node) => String(node.getAttribute("data-swarm-mcp-option") || "").trim())
        .filter(Boolean);
      Object.assign(draft, { workspaceRoot, objective, maxTasks: String(maxTasks), modelProvider, modelId, mcpServers });
      return { workspaceRoot, objective, maxTasks, modelProvider, modelId, mcpServers };
    };

    byId("swarm-root")?.addEventListener("input", (event) => setDraftValue("workspaceRoot", String(event.target?.value || "")));
    byId("swarm-objective")?.addEventListener("input", (event) => setDraftValue("objective", String(event.target?.value || "")));
    byId("swarm-max")?.addEventListener("input", (event) => setDraftValue("maxTasks", String(event.target?.value || "")));

    byId("swarm-model-provider")?.addEventListener("change", (event) => {
      const providerId = String(event.target?.value || "").trim();
      setDraftValue("modelProvider", providerId);
      const modelCandidates = providers.find((row) => row.id === providerId)?.models || [];
      const configuredDefault = String(providerConfig?.providers?.[providerId]?.default_model || "").trim();
      setDraftValue("modelId", modelCandidates.includes(configuredDefault) ? configuredDefault : modelCandidates[0] || "");
      renderSwarm(ctx, { force: true });
    });
    byId("swarm-model-id")?.addEventListener("change", (event) => setDraftValue("modelId", String(event.target?.value || "").trim()));

    viewEl.querySelectorAll("[data-swarm-mcp-option]").forEach((el) =>
      el.addEventListener("change", () => {
        const picked = [...viewEl.querySelectorAll("[data-swarm-mcp-option]:checked")]
          .map((node) => String(node.getAttribute("data-swarm-mcp-option") || "").trim())
          .filter(Boolean);
        setDraftValue("mcpServers", picked);
      })
    );

    byId("swarm-start")?.addEventListener("click", async () => {
      try {
        const current = collectCurrentFormState();
        const result = await api("/api/swarm/start", {
          method: "POST",
          body: JSON.stringify(current),
        });
        if (result?.runId) state.__swarmSelectedRunId = String(result.runId || "").trim();
        toast("ok", "Swarm context run created. Planning started.");
        renderSwarm(ctx, { force: true });
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });

    byId("swarm-approve")?.addEventListener("click", async () => {
      if (!selectedRunId) return;
      try {
        await api("/api/swarm/approve", { method: "POST", body: JSON.stringify({ runId: selectedRunId }) });
        toast("ok", "Plan approved. Execution started.");
        renderSwarm(ctx, { force: true });
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });

    byId("swarm-pause")?.addEventListener("click", async () => {
      if (!selectedRunId) return;
      try {
        await api("/api/swarm/pause", { method: "POST", body: JSON.stringify({ runId: selectedRunId }) });
        toast("ok", "Run paused.");
        renderSwarm(ctx, { force: true });
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });

    byId("swarm-resume")?.addEventListener("click", async () => {
      if (!selectedRunId) return;
      try {
        await api("/api/swarm/resume", { method: "POST", body: JSON.stringify({ runId: selectedRunId }) });
        toast("ok", "Run resumed.");
        renderSwarm(ctx, { force: true });
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });

    byId("swarm-cancel")?.addEventListener("click", async () => {
      if (!selectedRunId) return;
      try {
        await api("/api/swarm/cancel", { method: "POST", body: JSON.stringify({ runId: selectedRunId }) });
        toast("ok", "Run cancelled.");
        renderSwarm(ctx, { force: true });
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    });

    viewEl.querySelectorAll("[data-run-select]").forEach((button) =>
      button.addEventListener("click", () => {
        state.__swarmSelectedRunId = String(button.getAttribute("data-run-select") || "").trim();
        setSwarmRefreshLock(state, 800);
        renderSwarm(ctx, { force: true });
      })
    );

    viewEl.querySelectorAll("[data-step-copy]").forEach((button) =>
      button.addEventListener("click", async () => {
        try {
          await copyText(String(button.getAttribute("data-step-copy") || ""));
          toast("ok", "Identifiers copied.");
        } catch {
          toast("err", "Copy failed.");
        }
      })
    );

    viewEl.querySelectorAll("[data-step-retry]").forEach((button) =>
      button.addEventListener("click", async () => {
        const stepId = String(button.getAttribute("data-step-retry") || "").trim();
        if (!selectedRunId || !stepId) return;
        try {
          await api("/api/swarm/retry", {
            method: "POST",
            body: JSON.stringify({ runId: selectedRunId, stepId }),
          });
          toast("ok", `Retry requested for ${stepId}.`);
          renderSwarm(ctx, { force: true });
        } catch (e) {
          toast("err", e instanceof Error ? e.message : String(e));
        }
      })
    );

    viewEl.querySelectorAll("[data-step-open-session]").forEach((button) =>
      button.addEventListener("click", () => {
        const sessionId = String(button.getAttribute("data-step-open-session") || "").trim();
        if (!sessionId) return;
        state.currentSessionId = sessionId;
        if (typeof setRoute === "function") setRoute("chat");
      })
    );

    const poll = setInterval(() => {
      if (state.route !== "swarm") return;
      if (swarmFormHasFocus()) return;
      if (swarmRefreshLocked(state)) return;
      renderSwarm(ctx);
    }, 4000);

    const stopPoll = () => clearInterval(poll);
    state.__swarmLiveCleanup.push(stopPoll);
    addCleanup(stopPoll);

    const stopEvt = subscribeSse(
      `/api/swarm/events${selectedRunId ? `?runId=${encodeURIComponent(selectedRunId)}` : ""}`,
      () => {
        if (state.route !== "swarm") return;
        if (swarmFormHasFocus()) return;
        if (swarmRefreshLocked(state)) return;
        renderSwarm(ctx);
      }
    );
    state.__swarmLiveCleanup.push(stopEvt);
    addCleanup(stopEvt);
  } finally {
    state.__swarmRenderInFlight = false;
    state.__swarmRenderedOnce = true;
  }
}
