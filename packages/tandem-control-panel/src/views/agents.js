export async function renderAgents(ctx) {
  const { state, byId, toast, escapeHtml, api, renderIcons } = ctx;
  const [routinesRaw, automationsRaw] = await Promise.all([
    state.client.routines.list().catch(() => ({ routines: [] })),
    state.client.automations.list().catch(() => ({ automations: [] })),
  ]);
  const routines = routinesRaw.routines || [];
  const automations = automationsRaw.automations || [];

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
      <div class="tcp-list">${automations.map((r) => `<div class="tcp-list-item flex items-center justify-between gap-2"><span>${escapeHtml(r.name || r.id)}</span><span class="tcp-subtle">${escapeHtml(String(r.status || ""))}</span></div>`).join("") || '<p class="tcp-subtle">No automations.</p>'}</div>
    </div>
  `;

  const routineList = byId("routine-list");
  routineList.innerHTML =
    routines
      .map((r) => {
        const rid = routineKey(r);
        return `
      <div class="tcp-list-item flex items-center justify-between gap-3">
        <div>
          <div class="font-medium">${escapeHtml(r.name || rid || "Unnamed routine")}</div>
          <div class="tcp-subtle font-mono">${escapeHtml(formatSchedule(r.schedule))}</div>
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
        toast("ok", bits.length ? `Routine triggered (${bits.join(", ")}).` : "Routine triggered.");
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      } finally {
        b.disabled = false;
        b.innerHTML = prev;
        renderIcons(b);
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
