function pickStatusClass(status) {
  const normalized = String(status || "").toLowerCase();
  if (normalized.includes("fail") || normalized.includes("error")) return "tcp-badge-err";
  if (normalized.includes("wait") || normalized.includes("queue") || normalized.includes("new")) return "tcp-badge-warn";
  return "tcp-badge-ok";
}

function buildRoleSummary(tasks) {
  const summary = new Map();
  for (const t of tasks) {
    const role = t.ownerRole || "unknown";
    const rec = summary.get(role) || { total: 0, running: 0, done: 0, failed: 0 };
    rec.total += 1;
    const status = String(t.status || "").toLowerCase();
    if (status.includes("fail") || status.includes("error")) rec.failed += 1;
    else if (status.includes("done") || status.includes("complete")) rec.done += 1;
    else rec.running += 1;
    summary.set(role, rec);
  }
  return [...summary.entries()];
}

function reasonText(reason) {
  if (!reason) return "";
  if (reason.kind === "task_transition") return `${reason.taskId}: ${reason.from} -> ${reason.to} (${reason.reason || "status changed"})`;
  if (reason.kind === "task_reason") return `${reason.taskId}: ${reason.reason || "updated"}`;
  return reason.reason || JSON.stringify(reason);
}

export async function renderSwarm(ctx) {
  const { api, byId, escapeHtml, toast, state, addCleanup } = ctx;
  const status = await api("/api/swarm/status").catch(() => ({ status: "error" }));
  const snapshot = await api("/api/swarm/snapshot").catch(() => ({ registry: { value: { tasks: {} } }, logs: [], reasons: [] }));
  const tasks = Object.values(snapshot.registry?.value?.tasks || {});
  const reasons = (snapshot.reasons || []).slice().reverse();
  const roleSummary = buildRoleSummary(tasks);

  byId("view").innerHTML = `
    <div class="tcp-card">
      <div class="mb-3 flex items-center justify-between gap-3">
        <h3 class="tcp-title flex items-center gap-2"><i data-lucide="cpu"></i> Node Swarm Orchestrator</h3>
        <span class="${pickStatusClass(status.status)}">${escapeHtml(status.status || "idle")}</span>
      </div>
      <div class="grid gap-3 md:grid-cols-[1fr_1fr_120px_auto]">
        <input id="swarm-root" class="tcp-input" value="${escapeHtml(status.workspaceRoot || "")}" placeholder="workspace root" />
        <input id="swarm-objective" class="tcp-input" value="${escapeHtml(status.objective || "Ship a small feature end-to-end")}" placeholder="objective" />
        <input id="swarm-max" class="tcp-input" type="number" min="1" value="${escapeHtml(String(status.maxTasks || 3))}" />
        <div class="flex gap-2">
          <button id="swarm-start" class="tcp-btn-primary" ${status.localEngine ? "" : "disabled"}><i data-lucide="play"></i> Start</button>
          <button id="swarm-stop" class="tcp-btn-danger"><i data-lucide="square"></i> Stop</button>
        </div>
      </div>
      ${status.localEngine ? "" : '<p class="mt-3 rounded-xl border border-amber-700/60 bg-amber-950/20 px-3 py-2 text-sm text-amber-300">Swarm orchestration is disabled on remote engine URLs. Monitoring remains available.</p>'}
    </div>

    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Live Agent Flow</h3>
      <div class="overflow-hidden rounded-xl border border-slate-700 bg-black/20 p-2">
        <svg viewBox="0 0 680 240" class="h-[280px] w-full">
          <defs>
            <linearGradient id="wire-grad" x1="0" y1="0" x2="1" y2="0">
              <stop offset="0%" stop-color="#64748b" stop-opacity="0.12"></stop>
              <stop offset="50%" stop-color="#94a3b8" stop-opacity="0.95"></stop>
              <stop offset="100%" stop-color="#64748b" stop-opacity="0.12"></stop>
            </linearGradient>
          </defs>
          <line x1="140" y1="120" x2="320" y2="60" stroke="url(#wire-grad)" stroke-width="2" class="wire-line-active"></line>
          <line x1="140" y1="120" x2="320" y2="120" stroke="url(#wire-grad)" stroke-width="2" class="wire-line-active"></line>
          <line x1="140" y1="120" x2="320" y2="180" stroke="url(#wire-grad)" stroke-width="2" class="wire-line-active"></line>
          <line x1="360" y1="60" x2="540" y2="50" stroke="url(#wire-grad)" stroke-width="2" class="wire-line-active"></line>
          <line x1="360" y1="120" x2="540" y2="120" stroke="url(#wire-grad)" stroke-width="2" class="wire-line-active"></line>
          <line x1="360" y1="180" x2="540" y2="190" stroke="url(#wire-grad)" stroke-width="2" class="wire-line-active"></line>

          <circle cx="120" cy="120" r="20" fill="rgba(30,41,59,0.95)" stroke="rgba(148,163,184,0.8)" stroke-width="1.5" class="node-active"></circle>
          <text x="120" y="152" text-anchor="middle" fill="#cbd5e1" font-size="11">Manager</text>
          <circle cx="340" cy="60" r="18" fill="rgba(30,41,59,0.95)" stroke="rgba(148,163,184,0.7)" stroke-width="1.5" class="node-active"></circle>
          <text x="340" y="92" text-anchor="middle" fill="#cbd5e1" font-size="11">Worker</text>
          <circle cx="340" cy="120" r="18" fill="rgba(30,41,59,0.95)" stroke="rgba(148,163,184,0.7)" stroke-width="1.5" class="node-active"></circle>
          <text x="340" y="152" text-anchor="middle" fill="#cbd5e1" font-size="11">Tester</text>
          <circle cx="340" cy="180" r="18" fill="rgba(30,41,59,0.95)" stroke="rgba(148,163,184,0.7)" stroke-width="1.5" class="node-active"></circle>
          <text x="340" y="212" text-anchor="middle" fill="#cbd5e1" font-size="11">Reviewer</text>
        </svg>
      </div>
      <div class="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        ${roleSummary
          .map(
            ([role, rec]) =>
              `<div class="tcp-list-item"><div class="font-medium">${escapeHtml(role)}</div><div class="tcp-subtle mt-1">${rec.total} tasks</div><div class="text-xs text-slate-400">${rec.running} active / ${rec.done} done / ${rec.failed} failed</div></div>`
          )
          .join("") || '<p class="tcp-subtle">No active roles yet.</p>'}
      </div>
    </div>

    <div class="tcp-card">
      <div class="mb-3 flex items-center justify-between gap-2">
        <h3 class="tcp-title">Swarm Why Timeline</h3>
        <div class="flex gap-2">
          <select id="swarm-reason-kind" class="tcp-select !w-auto !py-1.5 text-xs">
            <option value="">All kinds</option>
            <option value="task_transition">task_transition</option>
            <option value="task_reason">task_reason</option>
          </select>
          <input id="swarm-reason-task" class="tcp-input !w-44 !py-1.5 text-xs" placeholder="Filter by task id" />
        </div>
      </div>
      <div id="swarm-reasons" class="grid max-h-[420px] gap-2 overflow-auto"></div>
    </div>

    <div class="grid gap-4 lg:grid-cols-2">
      <div class="tcp-card">
        <h3 class="tcp-title mb-3">Tasks (${tasks.length})</h3>
        <div id="swarm-tasks" class="grid max-h-[420px] gap-2 overflow-auto"></div>
      </div>
      <div class="tcp-card">
        <h3 class="tcp-title mb-3">Swarm Logs</h3>
        <pre id="swarm-logs" class="tcp-code max-h-[420px] overflow-auto"></pre>
      </div>
    </div>
  `;

  const taskList = byId("swarm-tasks");
  taskList.innerHTML =
    tasks
      .sort((a, b) => (b.lastUpdateMs || 0) - (a.lastUpdateMs || 0))
      .map(
        (t) => `<div class="tcp-list-item"><div class="flex items-center justify-between gap-2"><strong>${escapeHtml(t.taskId)}</strong><span class="${pickStatusClass(t.status)}">${escapeHtml(t.status || "unknown")}</span></div><div class="mt-1 text-xs uppercase tracking-wide text-slate-400">${escapeHtml(t.ownerRole || "")}</div><div class="tcp-subtle mt-1">${escapeHtml(t.statusReason || "")}</div></div>`
      )
      .join("") || '<p class="tcp-subtle">No swarm tasks yet.</p>';

  function renderReasons() {
    const kind = byId("swarm-reason-kind").value.trim();
    const taskFilter = byId("swarm-reason-task").value.trim().toLowerCase();
    const filtered = reasons.filter((r) => {
      if (kind && r.kind !== kind) return false;
      if (taskFilter && !String(r.taskId || "").toLowerCase().includes(taskFilter)) return false;
      return true;
    });

    byId("swarm-reasons").innerHTML =
      filtered
        .map(
          (r) => `
        <div class="tcp-list-item">
          <div class="flex items-center justify-between gap-2"><span class="text-xs text-slate-400">${new Date(r.at).toLocaleTimeString()}</span><span class="${pickStatusClass(r.to || r.from)}">${escapeHtml(r.kind || "reason")}</span></div>
          <div class="mt-1"><strong>${escapeHtml(r.taskId || "swarm")}</strong> <span class="tcp-subtle">${escapeHtml(r.role || "")}</span></div>
          <div class="mt-1 text-sm text-slate-300">${escapeHtml(reasonText(r))}</div>
        </div>
      `
        )
        .join("") || '<p class="tcp-subtle">No timeline reasons yet.</p>';
  }

  byId("swarm-reason-kind").addEventListener("change", renderReasons);
  byId("swarm-reason-task").addEventListener("input", renderReasons);
  renderReasons();

  byId("swarm-logs").textContent = (snapshot.logs || [])
    .slice(-200)
    .map((l) => `[${new Date(l.at).toLocaleTimeString()}] ${l.stream}: ${l.line}`)
    .join("\n");

  byId("swarm-start").addEventListener("click", async () => {
    try {
      await api("/api/swarm/start", {
        method: "POST",
        body: JSON.stringify({
          workspaceRoot: byId("swarm-root").value.trim(),
          objective: byId("swarm-objective").value.trim(),
          maxTasks: Number.parseInt(byId("swarm-max").value, 10) || 3,
        }),
      });
      toast("ok", "Swarm started.");
      renderSwarm(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });

  byId("swarm-stop").addEventListener("click", async () => {
    try {
      await api("/api/swarm/stop", { method: "POST" });
      toast("ok", "Swarm stop requested.");
      renderSwarm(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });

  const poll = setInterval(() => {
    if (state.route === "swarm") renderSwarm(ctx);
  }, 4000);
  addCleanup(() => clearInterval(poll));

  try {
    const evt = new EventSource("/api/swarm/events", { withCredentials: true });
    evt.onmessage = () => {
      if (state.route === "swarm") renderSwarm(ctx);
    };
    evt.onerror = () => evt.close();
    addCleanup(() => evt.close());
  } catch {
    // ignore
  }
}
