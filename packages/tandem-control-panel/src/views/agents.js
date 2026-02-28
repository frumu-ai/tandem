export async function renderAgents(ctx) {
  const { state, byId, toast, escapeHtml, api, renderIcons } = ctx;
  const [routinesRaw, automationsRaw, routineRunsRaw, automationRunsRaw] = await Promise.all([
    state.client.routines.list().catch(() => ({ routines: [] })),
    state.client.automations.list().catch(() => ({ automations: [] })),
    state.client.routines.listRuns({ limit: 100 }).catch(() => ({ runs: [] })),
    state.client.automations.listRuns({ limit: 100 }).catch(() => ({ runs: [] })),
  ]);
  const routines = routinesRaw.routines || [];
  const automations = automationsRaw.automations || [];
  const routineRuns = Array.isArray(routineRunsRaw?.runs) ? routineRunsRaw.runs : [];
  const automationRuns = Array.isArray(automationRunsRaw?.runs) ? automationRunsRaw.runs : [];

  const slugify = (value = "") =>
    String(value)
      .toLowerCase()
      .trim()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 64) || "routine";

  const formatSchedule = (schedule) => {
    if (!schedule) return "manual";
    if (typeof schedule === "string") return schedule;
    const intervalSeconds = Number(
      schedule?.interval_seconds?.seconds ??
        schedule?.intervalSeconds?.seconds ??
        schedule?.intervalSeconds ??
        0
    );
    if (intervalSeconds > 0) {
      if (intervalSeconds % 3600 === 0) return `every ${intervalSeconds / 3600}h`;
      if (intervalSeconds % 60 === 0) return `every ${intervalSeconds / 60}m`;
      return `every ${intervalSeconds}s`;
    }
    const cronExpr = String(
      schedule?.cron?.expression ??
        schedule?.cron?.cron ??
        schedule?.expression ??
        schedule?.cron ??
        ""
    ).trim();
    if (cronExpr) {
      const cron = cronExpr;
      const daily = cron.match(/^(\d{1,2})\s+(\d{1,2})\s+\*\s+\*\s+\*$/);
      if (daily) {
        const m = String(daily[1]).padStart(2, "0");
        const h = String(daily[2]).padStart(2, "0");
        return `daily ${h}:${m}`;
      }
      const weekly = cron.match(/^(\d{1,2})\s+(\d{1,2})\s+\*\s+\*\s+([0-6])$/);
      if (weekly) {
        const m = String(weekly[1]).padStart(2, "0");
        const h = String(weekly[2]).padStart(2, "0");
        const labels = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const day = labels[Number.parseInt(weekly[3], 10)] || weekly[3];
        return `weekly ${day} ${h}:${m}`;
      }
      return `cron ${cron}`.trim();
    }
    if (schedule.type === "manual") return "manual";
    return JSON.stringify(schedule);
  };

  const detectPromptFile = (routine) => {
    const argsPath = String(routine?.args?.promptFilePath || "").trim();
    if (argsPath) return argsPath;
    const entrypoint = String(routine?.entrypoint || routine?.prompt || "");
    const match = entrypoint.match(/(control-panel\/routines\/[A-Za-z0-9._\-\/]+\.md)/);
    return match?.[1] || "";
  };

  const routineKey = (routine) =>
    String(routine?.id || routine?.routine_id || routine?.routineID || routine?.routineId || "").trim();
  const automationKey = (automation) =>
    String(
      automation?.id ||
        automation?.automation_id ||
        automation?.automationID ||
        automation?.automationId ||
        ""
    ).trim();
  const runIdOf = (run) => String(run?.runId || run?.runID || run?.run_id || run?.id || "").trim();
  const runRoutineIdOf = (run) =>
    String(run?.routineId || run?.routine_id || run?.routineID || run?.routineId || "").trim();
  const runAutomationIdOf = (run) =>
    String(run?.automationId || run?.automation_id || run?.automationID || run?.automationId || "").trim();
  const runStatusOf = (run) => String(run?.status || "unknown").toLowerCase();
  const runDetailOf = (run) => String(run?.detail || run?.reason || "").trim();
  const firstTimestamp = (run) => {
    const candidates = [
      run?.updatedAtMs,
      run?.updated_at_ms,
      run?.finishedAtMs,
      run?.finished_at_ms,
      run?.startedAtMs,
      run?.started_at_ms,
      run?.createdAtMs,
      run?.created_at_ms,
      run?.firedAtMs,
      run?.fired_at_ms,
    ];
    for (const v of candidates) {
      const n = Number(v);
      if (Number.isFinite(n) && n > 0) return n;
    }
    return 0;
  };
  const formatTimestamp = (ts) => {
    const n = Number(ts);
    if (!Number.isFinite(n) || n <= 0) return "time n/a";
    return new Date(n).toLocaleString();
  };
  const truncate = (value, max = 120) => {
    const text = String(value || "").trim();
    if (!text) return "";
    return text.length > max ? `${text.slice(0, max - 1)}...` : text;
  };
  const runStatusClass = (status) => {
    const normalized = String(status || "").toLowerCase();
    if (normalized.includes("complete") || normalized.includes("succeed")) return "tcp-badge-info";
    if (normalized.includes("fail") || normalized.includes("cancel") || normalized.includes("deny"))
      return "tcp-badge-err";
    if (normalized.includes("block") || normalized.includes("approval") || normalized.includes("pause"))
      return "tcp-badge-warn";
    return "tcp-badge-info";
  };
  const latestRunsBy = (runs, idOf) => {
    const map = new Map();
    const sorted = [...runs].sort((a, b) => firstTimestamp(b) - firstTimestamp(a));
    for (const run of sorted) {
      const id = idOf(run);
      if (!id || map.has(id)) continue;
      map.set(id, run);
    }
    return map;
  };
  const routineNameById = new Map(
    routines
      .map((r) => [routineKey(r), String(r.name || routineKey(r) || "Routine").trim()])
      .filter(([id]) => !!id)
  );
  const automationNameById = new Map(
    automations
      .map((a) => [automationKey(a), String(a.name || automationKey(a) || "Automation").trim()])
      .filter(([id]) => !!id)
  );
  const latestRoutineRunById = latestRunsBy(routineRuns, runRoutineIdOf);
  const latestAutomationRunById = latestRunsBy(automationRuns, runAutomationIdOf);
  const recentRuns = [...routineRuns.map((run) => ({ family: "routine", run })), ...automationRuns.map((run) => ({ family: "automation", run }))]
    .sort((a, b) => firstTimestamp(b.run) - firstTimestamp(a.run))
    .slice(0, 30);
  const automationsMarkup =
    automations
      .map((a) => {
        const aid = automationKey(a);
        const latest = latestAutomationRunById.get(aid);
        const status = runStatusOf(latest);
        const runId = runIdOf(latest);
        const detail = truncate(runDetailOf(latest));
        return `<div class="tcp-list-item">
          <div class="flex items-center justify-between gap-2">
            <span>${escapeHtml(String(a.name || aid || "Automation"))}</span>
            <div class="flex items-center gap-2">
              <span class="tcp-subtle">${escapeHtml(String(a.status || ""))}</span>
              ${
                aid
                  ? `<button data-run-automation="${escapeHtml(aid)}" class="tcp-btn h-7 px-2 text-xs"><i data-lucide="play"></i> Run</button>`
                  : ""
              }
            </div>
          </div>
          ${
            latest
              ? `<div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
                <span class="${runStatusClass(status)}">${escapeHtml(status)}</span>
                <span class="tcp-subtle">${escapeHtml(formatTimestamp(firstTimestamp(latest)))}</span>
                <span class="tcp-subtle font-mono">${escapeHtml(runId || "run n/a")}</span>
              </div>
              ${detail ? `<div class="mt-1 text-xs text-slate-400">${escapeHtml(detail)}</div>` : ""}`
              : `<div class="mt-1 text-xs text-slate-500">No automation runs yet.</div>`
          }
        </div>`;
      })
      .join("") || '<p class="tcp-subtle">No automations.</p>';
  const recentRunsMarkup =
    recentRuns
      .map(({ family, run }) => {
        const isRoutine = family === "routine";
        const ownerId = isRoutine ? runRoutineIdOf(run) : runAutomationIdOf(run);
        const ownerName = isRoutine
          ? routineNameById.get(ownerId) || ownerId || "Routine"
          : automationNameById.get(ownerId) || ownerId || "Automation";
        const status = runStatusOf(run);
        const detail = truncate(runDetailOf(run), 180);
        return `<div class="tcp-list-item">
          <div class="flex items-center justify-between gap-2">
            <span class="font-medium">${escapeHtml(ownerName)}</span>
            <span class="${runStatusClass(status)}">${escapeHtml(status)}</span>
          </div>
          <div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
            <span class="tcp-subtle">${isRoutine ? "Routine" : "Automation"}</span>
            <span class="tcp-subtle">${escapeHtml(formatTimestamp(firstTimestamp(run)))}</span>
            <span class="tcp-subtle font-mono">${escapeHtml(runIdOf(run) || "run n/a")}</span>
          </div>
          ${detail ? `<div class="mt-1 text-xs text-slate-400">${escapeHtml(detail)}</div>` : ""}
        </div>`;
      })
      .join("") || '<p class="tcp-subtle">No runs yet.</p>';

  byId("view").innerHTML = `
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Create Routine</h3>
      <div class="grid gap-3 md:grid-cols-2">
        <input id="routine-name" class="tcp-input" placeholder="Routine name" />
        <select id="routine-schedule-mode" class="tcp-select">
          <option value="interval">Every X minutes/hours</option>
          <option value="daily">Daily at specific time</option>
          <option value="weekly">Weekly on a specific day/time</option>
          <option value="customCron">Custom cron</option>
          <option value="manual">Manual only</option>
        </select>
      </div>
      <div id="routine-interval-controls" class="mt-3 grid gap-3 md:grid-cols-2">
        <input id="routine-interval-value" class="tcp-input" type="number" min="1" max="10000" step="1" value="30" />
        <select id="routine-interval-unit" class="tcp-select">
          <option value="minutes">Minutes</option>
          <option value="hours">Hours</option>
        </select>
      </div>
      <div id="routine-daily-controls" class="mt-3 grid hidden gap-3 md:grid-cols-2">
        <input id="routine-time" class="tcp-input" type="time" value="09:00" />
        <div class="tcp-subtle self-center text-xs">Runs once per day at selected time</div>
      </div>
      <div id="routine-weekly-controls" class="mt-3 grid hidden gap-3 md:grid-cols-2">
        <select id="routine-weekday" class="tcp-select">
          <option value="1">Monday</option>
          <option value="2">Tuesday</option>
          <option value="3">Wednesday</option>
          <option value="4">Thursday</option>
          <option value="5">Friday</option>
          <option value="6">Saturday</option>
          <option value="0">Sunday</option>
        </select>
        <input id="routine-weekly-time" class="tcp-input" type="time" value="09:00" />
      </div>
      <div id="routine-cron-controls" class="mt-3 grid hidden gap-3 md:grid-cols-1">
        <input id="routine-cron" class="tcp-input hidden" placeholder="Cron e.g. 0 * * * *" />
      </div>
      <div class="mt-2 text-xs text-slate-400">
        <span id="routine-schedule-preview" class="font-mono">Schedule: every 30m</span>
      </div>
      <div class="mt-4 rounded-xl border border-slate-700/70 bg-slate-900/35 p-3">
        <div class="mb-2 flex items-center justify-between gap-2">
          <label class="inline-flex items-center gap-2 text-sm text-slate-200">
            <input id="routine-use-file" type="checkbox" class="h-4 w-4 accent-slate-400" checked />
            Save prompt as Markdown file (recommended)
          </label>
          <button id="save-routine-file" class="tcp-btn"><i data-lucide="save"></i> Save Prompt File</button>
        </div>
        <input id="routine-file-path" class="tcp-input mb-3 font-mono text-xs" value="control-panel/routines/new-routine.md" />
        <textarea id="routine-prompt" class="tcp-input" rows="6" placeholder="Write your routine instructions in Markdown..."></textarea>
        <div class="mt-2 text-xs text-slate-400">
          Tip: keep goals, constraints, and output format in this file so you can improve it anytime without rewriting the routine.
        </div>
      </div>
      <div class="mt-3 grid gap-2 md:grid-cols-[1fr_auto]">
        <div class="text-xs text-slate-400">Create will use the selected schedule and entry prompt.</div>
        <button id="create-routine" class="tcp-btn-primary"><i data-lucide="plus"></i> Create</button>
      </div>
    </div>
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Routines (${routines.length})</h3>
      <div id="routine-list" class="tcp-list"></div>
    </div>
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Automations (${automations.length})</h3>
      <div class="tcp-list">${automationsMarkup}</div>
    </div>
    <div class="tcp-card">
      <div class="mb-3 flex items-center justify-between gap-2">
        <h3 class="tcp-title">Recent Runs (${recentRuns.length})</h3>
        <button id="refresh-runs" class="tcp-btn"><i data-lucide="refresh-cw"></i> Refresh</button>
      </div>
      <div class="tcp-list">${recentRunsMarkup}</div>
    </div>
  `;

  const routineList = byId("routine-list");
  routineList.innerHTML =
    routines
      .map((r) => {
        const rid = routineKey(r);
        const latest = latestRoutineRunById.get(rid);
        const latestStatus = runStatusOf(latest);
        const latestRunId = runIdOf(latest);
        const latestDetail = truncate(runDetailOf(latest));
        return `
      <div class="tcp-list-item flex items-center justify-between gap-3">
        <div>
          <div class="font-medium">${escapeHtml(r.name || rid || "Unnamed routine")}</div>
          <div class="tcp-subtle font-mono">${escapeHtml(formatSchedule(r.schedule))}</div>
          ${
            latest
              ? `<div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
                <span class="${runStatusClass(latestStatus)}">${escapeHtml(latestStatus)}</span>
                <span class="tcp-subtle">${escapeHtml(formatTimestamp(firstTimestamp(latest)))}</span>
                <span class="tcp-subtle font-mono">${escapeHtml(latestRunId || "run n/a")}</span>
              </div>
              ${latestDetail ? `<div class="mt-1 text-xs text-slate-400">${escapeHtml(latestDetail)}</div>` : ""}`
              : `<div class="mt-1 text-xs text-slate-500">No runs yet.</div>`
          }
          ${
            detectPromptFile(r)
              ? `<div class="mt-1 text-xs text-slate-400 font-mono">${escapeHtml(detectPromptFile(r))}</div>`
              : ""
          }
        </div>
        <div class="flex gap-2">
          <button data-run="${escapeHtml(rid)}" class="tcp-btn"><i data-lucide="play"></i> Run</button>
          ${
            detectPromptFile(r)
              ? `<button data-edit-file="${escapeHtml(detectPromptFile(r))}" class="tcp-btn"><i data-lucide="folder-open"></i> Prompt File</button>`
              : ""
          }
          <button data-del="${escapeHtml(rid)}" class="tcp-btn-danger"><i data-lucide="trash-2"></i></button>
        </div>
      </div>`;
      })
      .join("") || '<p class="tcp-subtle">No routines.</p>';
  renderIcons(routineList);
  byId("refresh-runs").addEventListener("click", () => {
    renderAgents(ctx);
  });
  byId("view").querySelectorAll("[data-run-automation]").forEach((b) =>
    b.addEventListener("click", async () => {
      const automationId = String(b.dataset.runAutomation || "").trim();
      if (!automationId) {
        toast("err", "Automation ID is missing. Refresh and try again.");
        return;
      }
      const prev = b.innerHTML;
      b.disabled = true;
      b.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i> Running...';
      renderIcons(b);
      try {
        const response = await state.client.automations.runNow(automationId);
        const runId = String(response?.runId || "").trim();
        const status = String(response?.status || "").trim();
        const bits = [];
        if (runId) bits.push(`run ${runId}`);
        if (status) bits.push(`status ${status}`);
        toast(
          "ok",
          bits.length
            ? `Automation triggered (${bits.join(", ")}). It should move from queued to running within ~1 second.`
            : "Automation triggered."
        );
        setTimeout(() => {
          renderAgents(ctx);
        }, 500);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      } finally {
        if (b.isConnected) {
          b.disabled = false;
          b.innerHTML = prev;
          renderIcons(b);
        }
      }
    })
  );

  routineList.querySelectorAll("[data-run]").forEach((b) =>
    b.addEventListener("click", async () => {
      const routineId = String(b.dataset.run || "").trim();
      if (!routineId) {
        toast("err", "Routine ID is missing. Refresh and try again.");
        return;
      }
      const prev = b.innerHTML;
      b.disabled = true;
      b.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i> Running...';
      renderIcons(b);
      try {
        const response = await state.client.routines.runNow(routineId);
        const runId = String(response?.runId || "").trim();
        const status = String(response?.status || "").trim();
        const bits = [];
        if (runId) bits.push(`run ${runId}`);
        if (status) bits.push(`status ${status}`);
        toast(
          "ok",
          bits.length
            ? `Routine triggered (${bits.join(", ")}). It should move from queued to running within ~1 second.`
            : "Routine triggered."
        );
        setTimeout(() => {
          renderAgents(ctx);
        }, 500);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      } finally {
        if (b.isConnected) {
          b.disabled = false;
          b.innerHTML = prev;
          renderIcons(b);
        }
      }
    })
  );

  routineList.querySelectorAll("[data-del]").forEach((b) =>
    b.addEventListener("click", async () => {
      const routineId = String(b.dataset.del || "").trim();
      if (!routineId) {
        toast("err", "Routine ID is missing. Refresh and try again.");
        return;
      }
      try {
        await state.client.routines.delete(routineId);
        toast("ok", "Routine deleted.");
        renderAgents(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  routineList.querySelectorAll("[data-edit-file]").forEach((b) =>
    b.addEventListener("click", async () => {
      const path = String(b.dataset.editFile || "").trim();
      if (!path) return;
      try {
        const payload = await api(`/api/files/read?path=${encodeURIComponent(path)}`);
        byId("routine-use-file").checked = true;
        byId("routine-file-path").value = path;
        byId("routine-prompt").value = String(payload?.text || "");
        toast("ok", `Loaded prompt file: ${path}`);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );
  renderIcons(byId("view"));

  const scheduleModeEl = byId("routine-schedule-mode");
  const intervalControlsEl = byId("routine-interval-controls");
  const dailyControlsEl = byId("routine-daily-controls");
  const weeklyControlsEl = byId("routine-weekly-controls");
  const cronControlsEl = byId("routine-cron-controls");
  const intervalValueEl = byId("routine-interval-value");
  const intervalUnitEl = byId("routine-interval-unit");
  const timeEl = byId("routine-time");
  const weekdayEl = byId("routine-weekday");
  const weeklyTimeEl = byId("routine-weekly-time");
  const cronEl = byId("routine-cron");
  const schedulePreviewEl = byId("routine-schedule-preview");
  const nameEl = byId("routine-name");
  const useFileEl = byId("routine-use-file");
  const promptEl = byId("routine-prompt");
  const pathEl = byId("routine-file-path");

  const normalizeFilePath = () => {
    const fallback = `control-panel/routines/${slugify(nameEl.value || "new-routine")}.md`;
    const raw = String(pathEl.value || "").trim().replace(/\\/g, "/").replace(/^\/+/, "");
    const next = raw || fallback;
    const prefixed =
      next === "control-panel" || next.startsWith("control-panel/") ? next : `control-panel/${next}`;
    pathEl.value = prefixed.endsWith(".md") ? prefixed : `${prefixed}.md`;
  };

  const buildSchedule = () => {
    const mode = String(scheduleModeEl.value || "interval");
    if (mode === "interval") {
      const rawValue = Number.parseInt(String(intervalValueEl.value || "30"), 10);
      const safeValue = Number.isFinite(rawValue) ? rawValue : 30;
      if (safeValue <= 0) throw new Error("Interval must be at least 1.");
      const unit = String(intervalUnitEl.value || "minutes");
      const factor = unit === "hours" ? 3600 : 60;
      return { interval_seconds: { seconds: safeValue * factor } };
    }
    if (mode === "manual") {
      // Create as paused in submit handler; schedule value still must be valid.
      return { interval_seconds: { seconds: 24 * 3600 } };
    }
    if (mode === "daily") {
      const [hh, mm] = String(timeEl.value || "09:00")
        .split(":")
        .map((x) => Number.parseInt(x, 10));
      const h = Number.isFinite(hh) ? Math.min(23, Math.max(0, hh)) : 9;
      const m = Number.isFinite(mm) ? Math.min(59, Math.max(0, mm)) : 0;
      return { cron: { expression: `${m} ${h} * * *` } };
    }
    if (mode === "weekly") {
      const [hh, mm] = String(weeklyTimeEl.value || "09:00")
        .split(":")
        .map((x) => Number.parseInt(x, 10));
      const h = Number.isFinite(hh) ? Math.min(23, Math.max(0, hh)) : 9;
      const m = Number.isFinite(mm) ? Math.min(59, Math.max(0, mm)) : 0;
      const dow = Number.parseInt(String(weekdayEl.value || "1"), 10);
      const day = Number.isFinite(dow) ? Math.min(6, Math.max(0, dow)) : 1;
      return { cron: { expression: `${m} ${h} * * ${day}` } };
    }
    const cron = String(cronEl.value || "").trim();
    if (!cron) throw new Error("Custom cron is required.");
    return { cron: { expression: cron } };
  };

  const describeSchedule = () => {
    const mode = String(scheduleModeEl.value || "interval");
    if (mode === "interval") {
      const n = Number.parseInt(String(intervalValueEl.value || "30"), 10);
      const unit = String(intervalUnitEl.value || "minutes");
      const safeN = Number.isFinite(n) && n > 0 ? n : 30;
      const shortUnit = unit === "hours" ? "h" : "m";
      return `every ${safeN}${shortUnit}`;
    }
    if (mode === "daily") {
      return `daily at ${String(timeEl.value || "09:00")}`;
    }
    if (mode === "weekly") {
      const labels = {
        0: "Sunday",
        1: "Monday",
        2: "Tuesday",
        3: "Wednesday",
        4: "Thursday",
        5: "Friday",
        6: "Saturday",
      };
      const day = Number.parseInt(String(weekdayEl.value || "1"), 10);
      const dayLabel = labels[day] || "Monday";
      return `weekly on ${dayLabel} at ${String(weeklyTimeEl.value || "09:00")}`;
    }
    if (mode === "manual") return "manual";
    const cron = String(cronEl.value || "").trim();
    return cron ? `cron ${cron}` : "custom cron (required)";
  };

  const renderScheduleInputs = () => {
    const mode = String(scheduleModeEl.value || "interval");
    const showInterval = mode === "interval";
    const showDaily = mode === "daily";
    const showWeekly = mode === "weekly";
    const showCron = mode === "customCron";
    intervalControlsEl.classList.toggle("hidden", !showInterval);
    dailyControlsEl.classList.toggle("hidden", !showDaily);
    weeklyControlsEl.classList.toggle("hidden", !showWeekly);
    cronControlsEl.classList.toggle("hidden", !showCron);
    cronEl.classList.toggle("hidden", !showCron);
    try {
      const schedule = buildSchedule();
      schedulePreviewEl.textContent = `Schedule: ${describeSchedule()} (${formatSchedule(schedule)})`;
    } catch (e) {
      schedulePreviewEl.textContent = `Schedule: ${e instanceof Error ? e.message : String(e)}`;
    }
  };

  const savePromptFile = async () => {
    normalizeFilePath();
    const path = String(pathEl.value || "").trim();
    const text = String(promptEl.value || "");
    if (!path || !text.trim()) throw new Error("Prompt file path and content are required.");
    await api("/api/files/write", {
      method: "POST",
      body: JSON.stringify({ path, text, overwrite: true }),
    });
    return path;
  };

  byId("save-routine-file").addEventListener("click", async () => {
    try {
      const path = await savePromptFile();
      toast("ok", `Saved ${path}`);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });

  nameEl.addEventListener("input", () => {
    const current = String(pathEl.value || "").trim();
    if (!current || /new-routine\.md$/i.test(current)) normalizeFilePath();
  });
  scheduleModeEl.addEventListener("change", renderScheduleInputs);
  intervalValueEl.addEventListener("input", renderScheduleInputs);
  intervalUnitEl.addEventListener("change", renderScheduleInputs);
  timeEl.addEventListener("input", renderScheduleInputs);
  weekdayEl.addEventListener("change", renderScheduleInputs);
  weeklyTimeEl.addEventListener("input", renderScheduleInputs);
  cronEl.addEventListener("input", renderScheduleInputs);
  renderScheduleInputs();
  normalizeFilePath();

  byId("create-routine").addEventListener("click", async () => {
    try {
      const name = String(nameEl.value || "").trim();
      const prompt = String(promptEl.value || "").trim();
      if (!name || !prompt) throw new Error("Name and prompt are required.");
      const schedule = buildSchedule();
      const manualOnly = String(scheduleModeEl.value || "") === "manual";
      let entrypoint = prompt;
      let promptFilePath = "";
      if (useFileEl.checked) {
        promptFilePath = await savePromptFile();
        entrypoint = [
          `Use the routine prompt markdown at: ${promptFilePath}`,
          "Read the file first, then execute its instructions exactly.",
        ].join("\n");
      }
      const created = await state.client.routines.create({
        name,
        entrypoint,
        schedule,
        args: promptFilePath ? { promptFilePath } : {},
      });
      if (manualOnly) {
        const routineId = routineKey(created?.routine || created || {});
        if (routineId) {
          await state.client.routines.update(routineId, { status: "paused" });
        }
      }
      toast("ok", "Routine created.");
      renderAgents(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });
}
