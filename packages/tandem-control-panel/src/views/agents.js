export async function renderAgents(ctx) {
  const { state, byId, toast, escapeHtml, api, renderIcons } = ctx;
  const AGENTS_TABS = ["overview", "routines", "automations", "templates", "runs"];
  const AGENTS_WIZARD_SEEN_KEY = "tcp_agents_wizard_seen_v1";
  const parseAgentsUiState = () => {
    const hash = String(window.location.hash || "");
    const [, rawQuery = ""] = hash.split("?");
    const params = new URLSearchParams(rawQuery);
    const rawTab = String(params.get("tab") || "").trim().toLowerCase();
    const tab = AGENTS_TABS.includes(rawTab) ? rawTab : "overview";
    const rawFlow = String(params.get("flow") || "").trim().toLowerCase();
    const flow = rawFlow === "routine" ? "routine" : "advanced";
    const rawStep = Number.parseInt(String(params.get("step") || "0"), 10);
    const step = Number.isFinite(rawStep) ? Math.min(2, Math.max(0, rawStep)) : 0;
    const wizardProvided = params.has("wizard");
    const wizard = String(params.get("wizard") || "0").trim() === "1";
    return { tab, flow, step, wizard, wizardProvided };
  };
  const writeAgentsUiState = (patch = {}) => {
    const current = parseAgentsUiState();
    const next = {
      tab: AGENTS_TABS.includes(String(patch.tab || "").toLowerCase())
        ? String(patch.tab).toLowerCase()
        : current.tab,
      flow: String(patch.flow || current.flow).toLowerCase() === "routine" ? "routine" : "advanced",
      step: Number.isFinite(Number(patch.step))
        ? Math.min(2, Math.max(0, Number.parseInt(String(patch.step), 10) || 0))
        : current.step,
      wizard:
        typeof patch.wizard === "boolean"
          ? patch.wizard
          : String(patch.wizard || "").trim() === "1"
            ? true
            : patch.wizard === 0
              ? false
              : current.wizard,
    };
    const params = new URLSearchParams();
    params.set("tab", next.tab);
    params.set("flow", next.flow);
    params.set("step", String(next.step));
    params.set("wizard", next.wizard ? "1" : "0");
    const nextHash = `#/agents?${params.toString()}`;
    if (window.location.hash !== nextHash) window.location.hash = nextHash;
  };
  const [
    routinesRaw,
    automationsRaw,
    automationsV2Raw,
    routineRunsRaw,
    automationRunsRaw,
    providersCatalogRaw,
    providersConfigRaw,
    toolIdsRaw,
    mcpServersRaw,
    skillsRaw,
  ] = await Promise.all([
    state.client.routines.list().catch(() => ({ routines: [] })),
    state.client.automations.list().catch(() => ({ automations: [] })),
    state.client?.automationsV2?.list?.().catch(() => ({ automations: [] })) ||
      Promise.resolve({ automations: [] }),
    state.client.routines.listRuns({ limit: 100 }).catch(() => ({ runs: [] })),
    state.client.automations.listRuns({ limit: 100 }).catch(() => ({ runs: [] })),
    state.client.providers.catalog().catch(() => ({ all: [], connected: [], default: null })),
    state.client.providers.config().catch(() => ({ default: null, providers: {} })),
    state.client.listToolIds().catch(() => []),
    state.client?.mcp?.list?.().catch(() => ({})) || Promise.resolve({}),
    state.client?.skills?.list?.().catch(() => ({ skills: [] })) || Promise.resolve({ skills: [] }),
  ]);
  const routines = routinesRaw.routines || [];
  const automations = automationsRaw.automations || [];
  const automationsV2 = Array.isArray(automationsV2Raw?.automations) ? automationsV2Raw.automations : [];
  const routineRuns = Array.isArray(routineRunsRaw?.runs) ? routineRunsRaw.runs : [];
  const automationRuns = Array.isArray(automationRunsRaw?.runs) ? automationRunsRaw.runs : [];
  const providerCatalog = Array.isArray(providersCatalogRaw?.all) ? providersCatalogRaw.all : [];
  const providerConfigMap = providersConfigRaw?.providers || {};
  const toolIds = Array.isArray(toolIdsRaw)
    ? toolIdsRaw.map((x) => String(x || "").trim()).filter(Boolean).sort()
    : [];
  const normalizeMcpServers = (raw) => {
    if (!raw || typeof raw !== "object") return [];
    const entries = Array.isArray(raw)
      ? raw.map((x) => [String(x?.name || ""), x])
      : Object.entries(raw);
    return entries
      .map(([name, cfg]) => {
        const row = cfg && typeof cfg === "object" ? cfg : {};
        const serverName = String(row.name || name || "").trim();
        if (!serverName) return null;
        const connected = !!row.connected;
        const enabled = row.enabled !== false;
        return { name: serverName, connected, enabled };
      })
      .filter(Boolean)
      .sort((a, b) => a.name.localeCompare(b.name));
  };
  const mcpServers = normalizeMcpServers(mcpServersRaw);
  const connectedMcpServerNames = mcpServers
    .filter((s) => s.connected && s.enabled)
    .map((s) => s.name);
  const skillNames = Array.isArray(skillsRaw?.skills)
    ? skillsRaw.skills
        .map((s) => String(s?.name || s?.id || "").trim())
        .filter(Boolean)
        .sort((a, b) => a.localeCompare(b))
    : [];

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
  const detectRoutineModel = (routine) => {
    const spec = routine?.args?.model_policy?.default_model;
    const providerId = String(spec?.provider_id || "").trim();
    const modelId = String(spec?.model_id || "").trim();
    if (!providerId || !modelId) return "";
    return `${providerId}/${modelId}`;
  };
  const listFromRoutine = (routine, snake, camel) => {
    const raw = routine?.[snake] ?? routine?.[camel] ?? [];
    if (!Array.isArray(raw)) return [];
    const seen = new Set();
    const out = [];
    for (const item of raw) {
      const name = String(item || "").trim();
      if (!name) continue;
      if (seen.has(name)) continue;
      seen.add(name);
      out.push(name);
    }
    return out;
  };
  const boolFromRoutine = (routine, snake, camel, fallback) => {
    const raw = routine?.[snake];
    if (typeof raw === "boolean") return raw;
    const alt = routine?.[camel];
    if (typeof alt === "boolean") return alt;
    return fallback;
  };
  const normalizeToolList = (raw) => {
    const seen = new Set();
    const out = [];
    String(raw || "")
      .split(",")
      .map((x) => x.trim())
      .filter(Boolean)
      .forEach((name) => {
        if (seen.has(name)) return;
        seen.add(name);
        out.push(name);
      });
    return out;
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
  const isPendingApprovalStatus = (status) => {
    const normalized = String(status || "").toLowerCase();
    return normalized === "pending_approval" || normalized.includes("awaiting_approval");
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
  const routineById = new Map(
    routines
      .map((r) => [routineKey(r), r])
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
    .sort((a, b) => firstTimestamp(b.run) - firstTimestamp(a.run));
  const dedupedRecentRuns = [];
  const seenRunKeys = new Set();
  for (const entry of recentRuns) {
    const runId = runIdOf(entry.run);
    const ownerId = entry.family === "routine" ? runRoutineIdOf(entry.run) : runAutomationIdOf(entry.run);
    const key = runId || `${entry.family}:${ownerId}:${firstTimestamp(entry.run)}:${runStatusOf(entry.run)}`;
    if (seenRunKeys.has(key)) continue;
    seenRunKeys.add(key);
    dedupedRecentRuns.push(entry);
    if (dedupedRecentRuns.length >= 30) break;
  }
  const automationIds = new Set(automations.map((a) => automationKey(a)).filter(Boolean));
  const routineIds = new Set(routines.map((r) => routineKey(r)).filter(Boolean));
  const automationsMirrorRoutines =
    automationIds.size > 0 &&
    automationIds.size === routineIds.size &&
    [...automationIds].every((id) => routineIds.has(id));
  const providerDefaults = Object.fromEntries(
    Object.entries(providerConfigMap).map(([providerId, cfg]) => [
      providerId,
      String(cfg?.default_model || cfg?.defaultModel || "").trim(),
    ])
  );
  const providerIds = providerCatalog.map((p) => p.id).filter(Boolean);
  const modelIdsForProvider = (providerId) => {
    const entry = providerCatalog.find((p) => p.id === providerId);
    return Object.keys(entry?.models || {});
  };
  const configuredDefaultProvider = String(
    providersConfigRaw?.default || providersCatalogRaw?.default || state.providerDefault || ""
  ).trim();
  const initialProviderId = providerIds.includes(configuredDefaultProvider)
    ? configuredDefaultProvider
    : providerIds[0] || "";
  const configuredDefaultModel = String(
    providerDefaults[initialProviderId] || state.providerDefaultModel || ""
  ).trim();
  const initialModelCandidates = modelIdsForProvider(initialProviderId);
  const initialModelId = initialModelCandidates.includes(configuredDefaultModel)
    ? configuredDefaultModel
    : initialModelCandidates[0] || "";
  const providerOptionsMarkup =
    providerCatalog
      .map((p) => {
        const label = String(p.name || p.id || "").trim() || p.id;
        return `<option value="${escapeHtml(p.id)}" ${p.id === initialProviderId ? "selected" : ""}>${escapeHtml(label)}</option>`;
      })
      .join("") || '<option value="">No providers found</option>';
  const modelOptionsMarkup =
    initialModelCandidates
      .map((modelId) => `<option value="${escapeHtml(modelId)}" ${modelId === initialModelId ? "selected" : ""}>${escapeHtml(modelId)}</option>`)
      .join("") || '<option value="">No models found</option>';
  const toolOptionMarkup = toolIds.map((id) => `<option value="${escapeHtml(id)}"></option>`).join("");
  const automationsMarkup =
    automations
      .map((a) => {
        const aid = automationKey(a);
        const latest = latestAutomationRunById.get(aid);
        const status = runStatusOf(latest);
        const runId = runIdOf(latest);
        const detail = truncate(runDetailOf(latest));
        const needsReview = isPendingApprovalStatus(status) && !!runId;
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
              ${
                needsReview
                  ? `<div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
                  <button data-run-review="approve" data-run-id="${escapeHtml(runId)}" data-run-family="automation" class="tcp-btn h-7 px-2 text-xs">Approve</button>
                  <button data-run-review="deny" data-run-id="${escapeHtml(runId)}" data-run-family="automation" class="tcp-btn-danger h-7 px-2 text-xs">Deny</button>
                </div>`
                  : ""
              }
              ${detail ? `<div class="mt-1 text-xs text-slate-400">${escapeHtml(detail)}</div>` : ""}`
              : `<div class="mt-1 text-xs text-slate-500">No automation runs yet.</div>`
          }
        </div>`;
      })
      .join("") || '<p class="tcp-subtle">No automations.</p>';
  const automationV2Key = (automation) =>
    String(
      automation?.automation_id || automation?.automationId || automation?.id || automation?.automationID || ""
    ).trim();
  const automationsV2Markup =
    automationsV2
      .map((a) => {
        const aid = automationV2Key(a);
        const status = String(a?.status || "draft").toLowerCase();
        const nextFire = Number(a?.next_fire_at_ms || a?.nextFireAtMs || 0);
        const nodeCount = Array.isArray(a?.flow?.nodes) ? a.flow.nodes.length : 0;
        const agentCount = Array.isArray(a?.agents) ? a.agents.length : 0;
        return `<div class="tcp-list-item">
          <div class="flex items-center justify-between gap-2">
            <span>${escapeHtml(String(a?.name || aid || "Automation"))}</span>
            <div class="flex items-center gap-2">
              <span class="${runStatusClass(status)}">${escapeHtml(status)}</span>
              ${
                aid
                  ? `<button data-v2-run-now="${escapeHtml(aid)}" class="tcp-btn h-7 px-2 text-xs"><i data-lucide="play"></i> Run</button>`
                  : ""
              }
              ${
                aid
                  ? `<button data-v2-toggle="${escapeHtml(aid)}" data-v2-next="${status === "paused" ? "resume" : "pause"}" class="tcp-btn h-7 px-2 text-xs">
                    <i data-lucide="${status === "paused" ? "play-circle" : "pause-circle"}"></i> ${
                      status === "paused" ? "Resume" : "Pause"
                    }
                  </button>`
                  : ""
              }
              ${
                aid
                  ? `<button data-v2-runs="${escapeHtml(aid)}" class="tcp-btn h-7 px-2 text-xs"><i data-lucide="list"></i> Runs</button>`
                  : ""
              }
            </div>
          </div>
          <div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
            <span class="tcp-subtle">${agentCount} agents</span>
            <span class="tcp-subtle">${nodeCount} nodes</span>
            <span class="tcp-subtle">${nextFire > 0 ? formatTimestamp(nextFire) : "manual schedule"}</span>
            <span class="tcp-subtle font-mono">${escapeHtml(aid || "id n/a")}</span>
          </div>
        </div>`;
      })
      .join("") || '<p class="tcp-subtle">No advanced automations.</p>';
  const recentRunsMarkup =
    dedupedRecentRuns
      .map(({ family, run }) => {
        const isRoutine = family === "routine";
        const ownerId = isRoutine ? runRoutineIdOf(run) : runAutomationIdOf(run);
        const rid = runIdOf(run);
        const ownerName = isRoutine
          ? routineNameById.get(ownerId) || ownerId || "Routine"
          : automationNameById.get(ownerId) || ownerId || "Automation";
        const status = runStatusOf(run);
        const needsReview = isPendingApprovalStatus(status) && !!rid;
        const detail = truncate(runDetailOf(run), 180);
        return `<div class="tcp-list-item">
          <div class="flex items-center justify-between gap-2">
            <span class="font-medium">${escapeHtml(ownerName)}</span>
            <div class="flex items-center gap-2">
              ${
                rid
                  ? `<button data-inspect-run="${escapeHtml(rid)}" data-run-family="${escapeHtml(family)}" class="tcp-btn h-7 px-2 text-xs">Details</button>`
                  : ""
              }
              ${
                needsReview
                  ? `<button data-run-review="approve" data-run-id="${escapeHtml(rid)}" data-run-family="${escapeHtml(family)}" class="tcp-btn h-7 px-2 text-xs">Approve</button>
                <button data-run-review="deny" data-run-id="${escapeHtml(rid)}" data-run-family="${escapeHtml(family)}" class="tcp-btn-danger h-7 px-2 text-xs">Deny</button>`
                  : ""
              }
              <span class="${runStatusClass(status)}">${escapeHtml(status)}</span>
            </div>
          </div>
          <div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
            <span class="tcp-subtle">${isRoutine ? "Routine" : "Automation"}</span>
            <span class="tcp-subtle">${escapeHtml(formatTimestamp(firstTimestamp(run)))}</span>
            <span class="tcp-subtle font-mono">${escapeHtml(rid || "run n/a")}</span>
          </div>
          ${detail ? `<div class="mt-1 text-xs text-slate-400">${escapeHtml(detail)}</div>` : ""}
        </div>`;
      })
      .join("") || '<p class="tcp-subtle">No runs yet.</p>';
  const runTotalTokens = (run) => {
    const n = Number(run?.total_tokens ?? run?.totalTokens ?? 0);
    return Number.isFinite(n) && n >= 0 ? n : 0;
  };
  const runEstimatedCost = (run) => {
    const n = Number(run?.estimated_cost_usd ?? run?.estimatedCostUsd ?? 0);
    return Number.isFinite(n) && n >= 0 ? n : 0;
  };
  const now = Date.now();
  const last24hCutoff = now - 24 * 3600000;
  let tokens24h = 0;
  let cost24h = 0;
  const costByOwner = new Map();
  for (const run of [...routineRuns, ...automationRuns]) {
    const ts = firstTimestamp(run);
    const tokens = runTotalTokens(run);
    const cost = runEstimatedCost(run);
    if (ts >= last24hCutoff) {
      tokens24h += tokens;
      cost24h += cost;
    }
    const owner = runRoutineIdOf(run) || runAutomationIdOf(run) || "unknown";
    const row = costByOwner.get(owner) || { cost: 0 };
    row.cost += cost;
    costByOwner.set(owner, row);
  }
  const topCostRows = [...costByOwner.entries()]
    .map(([id, row]) => ({ id, cost: row.cost }))
    .sort((a, b) => b.cost - a.cost)
    .slice(0, 3);
  const uiState = parseAgentsUiState();
  if (
    !uiState.wizard &&
    !uiState.wizardProvided &&
    !routines.length &&
    !automations.length &&
    !automationsV2.length
  ) {
    let seen = false;
    try {
      seen = localStorage.getItem(AGENTS_WIZARD_SEEN_KEY) === "1";
    } catch {
      seen = false;
    }
    if (!seen) {
      setTimeout(() => {
        writeAgentsUiState({ tab: "overview", wizard: true, flow: "advanced", step: 0 });
      }, 0);
    }
  }
  const panelClass = (...tabs) => (tabs.includes(uiState.tab) ? "" : " hidden");
  const wizardStepLabels =
    uiState.flow === "routine"
      ? ["Choose flow", "Configure routine", "Run + monitor"]
      : ["Choose flow", "Configure agents", "Run + monitor"];

  byId("view").innerHTML = `
    <div class="tcp-card" data-agents-panel="header">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 class="tcp-title">Automations</h3>
          <p class="tcp-subtle text-xs">Build, schedule, and operate routines and multi-agent automations.</p>
        </div>
        <button id="agents-launch-wizard" class="tcp-btn-primary"><i data-lucide="sparkles"></i> Launch Walkthrough</button>
      </div>
      <div class="agents-tabs mt-3">
        <button class="agents-tab-btn ${uiState.tab === "overview" ? "active" : ""}" data-agents-tab="overview">Overview</button>
        <button class="agents-tab-btn ${uiState.tab === "routines" ? "active" : ""}" data-agents-tab="routines">Routines</button>
        <button class="agents-tab-btn ${uiState.tab === "automations" ? "active" : ""}" data-agents-tab="automations">Automations</button>
        <button class="agents-tab-btn ${uiState.tab === "templates" ? "active" : ""}" data-agents-tab="templates">Templates</button>
        <button class="agents-tab-btn ${uiState.tab === "runs" ? "active" : ""}" data-agents-tab="runs">Runs & Approvals</button>
      </div>
    </div>
    <div class="tcp-card${panelClass("overview")}" data-agents-panel="overview">
      <h3 class="tcp-title mb-2">Overview</h3>
      <div class="dashboard-kpis mb-3">
        <div><span class="dashboard-kpi-label">Routines</span><strong>${routines.length}</strong></div>
        <div><span class="dashboard-kpi-label">Automations</span><strong>${automationsV2.length}</strong></div>
        <div><span class="dashboard-kpi-label">Pending approvals</span><strong>${dedupedRecentRuns.filter((x) => isPendingApprovalStatus(runStatusOf(x.run))).length}</strong></div>
        <div><span class="dashboard-kpi-label">Recent runs</span><strong>${dedupedRecentRuns.length}</strong></div>
      </div>
      <div class="rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
        <div class="mb-2 flex items-center justify-between gap-2">
          <span class="font-medium">Automations + Cost (24h)</span>
          <span class="tcp-subtle text-xs">dashboard has full analytics</span>
        </div>
        <div class="grid gap-2 md:grid-cols-3">
          <div class="tcp-subtle">Tokens: <span class="font-mono text-slate-200">${tokens24h.toLocaleString()}</span></div>
          <div class="tcp-subtle">Estimated cost: <span class="font-mono text-slate-200">$${cost24h.toFixed(4)}</span></div>
          <div class="tcp-subtle">Top owners: <span class="font-mono text-slate-200">${escapeHtml(topCostRows.map((x) => x.id).join(", ") || "none")}</span></div>
        </div>
      </div>
    </div>
    <div class="tcp-card${uiState.wizard ? "" : " hidden"}" data-agents-panel="wizard">
      <div class="flex items-center justify-between gap-2">
        <h3 class="tcp-title">Walkthrough Wizard</h3>
        <button id="agents-wizard-close" class="tcp-btn">Close</button>
      </div>
      <div class="agents-steps mt-3">
        ${wizardStepLabels
          .map(
            (label, index) =>
              `<span class="agents-step-chip ${uiState.step === index ? "active" : ""}">${index + 1}. ${escapeHtml(label)}</span>`
          )
          .join("")}
      </div>
      <div class="mt-3 rounded-xl border border-slate-700/60 bg-slate-900/35 p-3">
        ${
          uiState.step === 0
            ? `<p class="tcp-subtle mb-3">Pick what you want to build first.</p>
               <div class="flex flex-wrap gap-2">
                 <button data-wizard-flow="advanced" class="tcp-btn ${uiState.flow === "advanced" ? "border-slate-300/80" : ""}">Advanced automation</button>
                 <button data-wizard-flow="routine" class="tcp-btn ${uiState.flow === "routine" ? "border-slate-300/80" : ""}">Routine</button>
               </div>`
            : uiState.step === 1
              ? `<p class="tcp-subtle mb-3">${
                  uiState.flow === "routine"
                    ? "Configure schedule, model, and policy for your routine."
                    : "Configure schedule, per-agent models/skills, and DAG nodes."
                }</p>
                 <button id="agents-wizard-open-builder" class="tcp-btn-primary">${
                   uiState.flow === "routine" ? "Open Routine Builder" : "Open Automation Builder"
                 }</button>`
              : `<p class="tcp-subtle mb-3">Launch runs and monitor approvals, pause/resume, and outcomes.</p>
                 <button id="agents-wizard-open-runs" class="tcp-btn-primary">Open Runs & Approvals</button>`
        }
      </div>
      <div class="mt-3 flex items-center justify-between">
        <button id="agents-wizard-prev" class="tcp-btn" ${uiState.step <= 0 ? "disabled" : ""}>Back</button>
        <button id="agents-wizard-next" class="tcp-btn-primary">${uiState.step >= 2 ? "Finish" : "Next"}</button>
      </div>
    </div>
    <div class="tcp-card${panelClass("templates")}" data-agents-panel="templates">
      <h3 class="tcp-title mb-3">Automation Templates</h3>
      <div class="grid gap-2 md:grid-cols-2">
        <button class="tcp-btn justify-start" data-template-id="github_bug_hunter">GitHub bug hunter</button>
        <button class="tcp-btn justify-start" data-template-id="code_generation_pipeline">Code generation pipeline</button>
        <button class="tcp-btn justify-start" data-template-id="release_notes_changelog">Release notes + changelog</button>
        <button class="tcp-btn justify-start" data-template-id="marketing_content_engine">Marketing content engine</button>
        <button class="tcp-btn justify-start" data-template-id="sales_lead_outreach">Sales lead outreach</button>
        <button class="tcp-btn justify-start" data-template-id="productivity_inbox_to_tasks">Productivity: inbox to tasks</button>
      </div>
      <p class="tcp-subtle mt-3 text-xs">Selecting a template pre-fills the advanced automation builder and opens the walkthrough.</p>
    </div>
    <div class="tcp-card${panelClass("routines")}" data-agents-panel="routines">
      <h3 class="tcp-title mb-3">Create Routine</h3>
      <p id="routine-form-mode" class="mb-2 text-xs text-slate-400">Creating new routine</p>
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
      <div class="mt-3 grid gap-3 md:grid-cols-2">
        <div>
          <label class="mb-1 block text-sm text-slate-300">Routine Provider</label>
          <select id="routine-model-provider" class="tcp-select" ${providerCatalog.length ? "" : "disabled"}>
            ${providerOptionsMarkup}
          </select>
        </div>
        <div>
          <label class="mb-1 block text-sm text-slate-300">Routine Model</label>
          <select id="routine-model-id" class="tcp-select" ${initialModelCandidates.length ? "" : "disabled"}>
            ${modelOptionsMarkup}
          </select>
        </div>
      </div>
      <div class="mt-1 text-xs text-slate-400">
        Model route for this routine: <span id="routine-model-preview" class="font-mono">${escapeHtml(initialProviderId && initialModelId ? `${initialProviderId}/${initialModelId}` : "default engine route")}</span>
      </div>
      <div class="mt-3 rounded-xl border border-slate-700/70 bg-slate-900/35 p-3">
        <label class="mb-2 inline-flex items-center gap-2 text-xs text-slate-200">
          <input id="routine-allow-everything" type="checkbox" class="h-4 w-4 accent-slate-400" />
          Allow everything (no approval, all tools, external integrations)
        </label>
        <label class="mb-1 block text-sm text-slate-300">Tool Allowlist (optional)</label>
        <input
          id="routine-allowed-tools"
          list="routine-tool-options"
          class="tcp-input font-mono text-xs"
          placeholder="Comma-separated tool IDs (leave empty to allow all tools by policy)"
        />
        <datalist id="routine-tool-options">${toolOptionMarkup}</datalist>
        <div id="routine-tool-scope-preview" class="mt-1 text-xs text-slate-400">Tool scope: all tools allowed by policy</div>
        <div class="mt-2 grid gap-2 md:grid-cols-2">
          <label class="inline-flex items-center gap-2 text-xs text-slate-300">
            <input id="routine-requires-approval" type="checkbox" class="h-4 w-4 accent-slate-400" checked />
            Require approval for external side effects
          </label>
          <label class="inline-flex items-center gap-2 text-xs text-slate-300">
            <input id="routine-external-integrations" type="checkbox" class="h-4 w-4 accent-slate-400" />
            Allow external integrations (MCP/connectors)
          </label>
        </div>
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
        <div class="flex items-center justify-end gap-2">
          <button id="cancel-edit-routine" class="tcp-btn hidden">Cancel Edit</button>
          <button id="create-routine" class="tcp-btn-primary"><i data-lucide="plus"></i> Create</button>
        </div>
      </div>
    </div>
    <div class="tcp-card${panelClass("routines")}" data-agents-panel="routines">
      <h3 class="tcp-title mb-3">Routines (${routines.length})</h3>
      <div id="routine-list" class="tcp-list"></div>
    </div>
    <div class="tcp-card${panelClass("automations")}" data-agents-panel="automations">
      <h3 class="tcp-title mb-3">Automations (${automations.length})</h3>
      ${
        automationsMirrorRoutines
          ? `<p class="tcp-subtle mb-2">Automation endpoints currently mirror routine records in this workspace.</p>`
          : ""
      }
      <div class="tcp-list">${automationsMarkup}</div>
    </div>
    <div class="tcp-card${panelClass("automations")}" data-agents-panel="automations">
      <h3 class="tcp-title mb-3">Automation Builder</h3>
      <div class="grid gap-3 md:grid-cols-2">
        <input id="automation-v2-name" class="tcp-input" placeholder="Automation name" />
        <input id="automation-v2-description" class="tcp-input" placeholder="Description (optional)" />
      </div>
      <div class="mt-3 grid gap-3 md:grid-cols-3">
        <select id="automation-v2-schedule-type" class="tcp-select">
          <option value="manual">Manual</option>
          <option value="interval">Interval</option>
          <option value="cron">Cron</option>
        </select>
        <input id="automation-v2-interval-seconds" class="tcp-input" type="number" min="1" value="3600" placeholder="Interval seconds" />
        <input id="automation-v2-cron" class="tcp-input" placeholder="Cron expression (UTC by default)" />
      </div>
      <div class="mt-3 grid gap-3 md:grid-cols-3">
        <input id="automation-v2-timezone" class="tcp-input" value="UTC" />
        <select id="automation-v2-misfire" class="tcp-select">
          <option value="run_once">run_once</option>
          <option value="skip">skip</option>
          <option value="catch_up">catch_up</option>
        </select>
        <input id="automation-v2-agent-count" class="tcp-input" type="number" min="1" max="12" value="2" />
      </div>
      <div class="mt-3 grid gap-3 md:grid-cols-[1fr_auto]">
        <select id="automation-v2-preset" class="tcp-select">
          <option value="">Choose preset...</option>
          <option value="github_bug_hunter">GitHub bug hunter</option>
          <option value="code_generation_pipeline">Code generation pipeline</option>
          <option value="release_notes_changelog">Release notes + changelog</option>
          <option value="marketing_content_engine">Marketing content engine</option>
          <option value="sales_lead_outreach">Sales lead outreach</option>
          <option value="productivity_inbox_to_tasks">Productivity: inbox to tasks</option>
        </select>
        <button id="automation-v2-apply-preset" class="tcp-btn"><i data-lucide="sparkles"></i> Apply Preset</button>
      </div>
      <div class="mt-2 text-xs text-slate-400">Per-agent model routing: choose provider + model per agent. Defaults come from Settings. Use “Custom” only when needed.</div>
      <div class="mt-3">
        <button id="automation-v2-generate-agents" class="tcp-btn"><i data-lucide="users"></i> Generate Agent Rows</button>
      </div>
      <datalist id="automation-v2-skill-options">${skillNames
        .map((name) => `<option value="${escapeHtml(name)}"></option>`)
        .join("")}</datalist>
      <datalist id="automation-v2-tool-options">${toolOptionMarkup}</datalist>
      <div id="automation-v2-agents-editor" class="mt-3 grid gap-2"></div>
      <div class="mt-4">
        <div class="mb-2 flex items-center justify-between">
          <h4 class="text-sm font-semibold text-slate-200">Flow Nodes (DAG)</h4>
          <button id="automation-v2-add-node" class="tcp-btn h-7 px-2 text-xs"><i data-lucide="plus"></i> Add Node</button>
        </div>
        <div id="automation-v2-nodes-editor" class="grid gap-2"></div>
      </div>
      <div class="mt-4 flex items-center justify-end gap-2">
        <button id="automation-v2-create" class="tcp-btn-primary"><i data-lucide="save"></i> Create Automation</button>
      </div>
    </div>
    <div class="tcp-card${panelClass("automations")}" data-agents-panel="automations">
      <h3 class="tcp-title mb-3">Advanced Automations (${automationsV2.length})</h3>
      <div class="tcp-list">${automationsV2Markup}</div>
    </div>
    <div class="tcp-card${panelClass("automations")}" data-agents-panel="automations">
      <h3 class="tcp-title mb-2">Automation Run Inspector</h3>
      <div id="automation-v2-run-inspector" class="tcp-list">
        <p class="tcp-subtle">Click an automation "Runs" button to inspect and control run state.</p>
      </div>
    </div>
    <div class="tcp-card${panelClass("runs")}" data-agents-panel="runs">
      <div class="mb-3 flex items-center justify-between gap-2">
        <h3 class="tcp-title">Recent Runs (${dedupedRecentRuns.length})</h3>
        <button id="refresh-runs" class="tcp-btn"><i data-lucide="refresh-cw"></i> Refresh</button>
      </div>
      <div class="tcp-list">${recentRunsMarkup}</div>
    </div>
    <div class="tcp-card${panelClass("runs")}" data-agents-panel="runs">
      <h3 class="tcp-title mb-2">Run Inspector</h3>
      <div id="run-inspector" class="tcp-list">
        <p class="tcp-subtle">Pick any recent run and click Details to inspect status, full detail, and artifacts.</p>
      </div>
    </div>
  `;

  byId("view")
    .querySelectorAll("[data-agents-tab]")
    .forEach((btn) =>
      btn.addEventListener("click", () => {
        const tab = String(btn.getAttribute("data-agents-tab") || "").trim().toLowerCase();
        if (!AGENTS_TABS.includes(tab)) return;
        writeAgentsUiState({ tab });
      })
    );
  byId("agents-launch-wizard")?.addEventListener("click", () => {
    writeAgentsUiState({ wizard: true, step: 0 });
  });
  byId("agents-wizard-close")?.addEventListener("click", () => {
    try {
      localStorage.setItem(AGENTS_WIZARD_SEEN_KEY, "1");
    } catch {
      // ignore storage failures
    }
    writeAgentsUiState({ wizard: false });
  });
  byId("agents-wizard-prev")?.addEventListener("click", () => {
    writeAgentsUiState({ step: Math.max(0, uiState.step - 1) });
  });
  byId("agents-wizard-next")?.addEventListener("click", () => {
    if (uiState.step >= 2) {
      try {
        localStorage.setItem(AGENTS_WIZARD_SEEN_KEY, "1");
      } catch {
        // ignore storage failures
      }
      writeAgentsUiState({ wizard: false, tab: "runs" });
      return;
    }
    writeAgentsUiState({ step: Math.min(2, uiState.step + 1) });
  });
  byId("view")
    .querySelectorAll("[data-wizard-flow]")
    .forEach((btn) =>
      btn.addEventListener("click", () => {
        const flow = String(btn.getAttribute("data-wizard-flow") || "").trim().toLowerCase();
        writeAgentsUiState({ flow: flow === "routine" ? "routine" : "advanced" });
      })
    );
  byId("agents-wizard-open-builder")?.addEventListener("click", () => {
    writeAgentsUiState({ tab: uiState.flow === "routine" ? "routines" : "automations", step: 2 });
  });
  byId("agents-wizard-open-runs")?.addEventListener("click", () => {
    writeAgentsUiState({ tab: "runs", step: 2 });
  });
  const routineList = byId("routine-list");
  routineList.innerHTML =
    routines
      .map((r) => {
        const rid = routineKey(r);
        const latest = latestRoutineRunById.get(rid);
        const latestStatus = runStatusOf(latest);
        const latestRunId = runIdOf(latest);
        const latestDetail = truncate(runDetailOf(latest));
        const routineModel = detectRoutineModel(r);
        const routineStatus = String(r.status || "active").toLowerCase();
        const allowedTools = listFromRoutine(r, "allowed_tools", "allowedTools");
        const requiresApproval = boolFromRoutine(r, "requires_approval", "requiresApproval", true);
        const externalAllowed = boolFromRoutine(
          r,
          "external_integrations_allowed",
          "externalIntegrationsAllowed",
          false
        );
        const isPaused = routineStatus === "paused";
        const needsReview = isPendingApprovalStatus(latestStatus) && !!latestRunId;
        return `
      <div class="tcp-list-item flex items-center justify-between gap-3">
        <div>
          <div class="font-medium">${escapeHtml(r.name || rid || "Unnamed routine")}</div>
          <div class="mt-1 flex items-center gap-2">
            <span class="${isPaused ? "tcp-badge-warn" : "tcp-badge-info"}">${escapeHtml(routineStatus)}</span>
            <span class="tcp-subtle font-mono">${escapeHtml(formatSchedule(r.schedule))}</span>
          </div>
          <div class="mt-1 text-xs text-slate-400 font-mono">${escapeHtml(routineModel || "default engine route")}</div>
          <div class="mt-1 text-xs text-slate-400">
            tools: ${escapeHtml(allowedTools.length ? allowedTools.join(", ") : "all (no explicit allowlist)")}
          </div>
          <div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
            <span class="${requiresApproval ? "tcp-badge-warn" : "tcp-badge-info"}">${requiresApproval ? "approval required" : "no approval gate"}</span>
            <span class="${externalAllowed ? "tcp-badge-info" : "tcp-badge-warn"}">${externalAllowed ? "external integrations allowed" : "external integrations blocked"}</span>
          </div>
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
            needsReview
              ? `<button data-run-review="approve" data-run-id="${escapeHtml(latestRunId)}" data-run-family="routine" class="tcp-btn">Approve</button>
          <button data-run-review="deny" data-run-id="${escapeHtml(latestRunId)}" data-run-family="routine" class="tcp-btn-danger">Deny</button>`
              : ""
          }
          <button data-toggle-status="${escapeHtml(rid)}" data-next-status="${isPaused ? "active" : "paused"}" class="tcp-btn">
            <i data-lucide="${isPaused ? "play-circle" : "pause-circle"}"></i> ${isPaused ? "Resume" : "Pause"}
          </button>
          <button data-edit-routine="${escapeHtml(rid)}" class="tcp-btn"><i data-lucide="pencil"></i> Edit</button>
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
  const runInspectorEl = byId("run-inspector");
  const v2RunInspectorEl = byId("automation-v2-run-inspector");
  const automationV2Api = state.client?.automationsV2;
  const v2Enabled = !!automationV2Api;
  if (!v2Enabled) {
    v2RunInspectorEl.innerHTML =
      '<p class="tcp-subtle">Advanced automation client API is unavailable in this build.</p>';
  }
  const v2Request = async (fn) => {
    if (!v2Enabled) throw new Error("Advanced automation API unavailable.");
    return fn(automationV2Api);
  };
  const providerSelectOptionsMarkup = [
    `<option value="">Default from settings</option>`,
    ...providerIds.map((id) => `<option value="${escapeHtml(id)}">${escapeHtml(id)}</option>`),
    `<option value="__custom__">Custom provider...</option>`,
  ].join("");
  const v2AgentRowMarkup = (index, seed = {}) => `
    <div data-v2-agent-row="${index}" class="rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
      <div class="mb-2 text-xs text-slate-300 font-semibold">Agent ${index + 1}</div>
      <div class="grid gap-2 md:grid-cols-2">
        <input data-v2-agent-field="agent_id" data-v2-agent-index="${index}" class="tcp-input font-mono text-xs" value="${escapeHtml(String(seed.agent_id || `agent-${index + 1}`))}" />
        <input data-v2-agent-field="display_name" data-v2-agent-index="${index}" class="tcp-input" placeholder="Display name" value="${escapeHtml(String(seed.display_name || `Agent ${index + 1}`))}" />
        <select data-v2-agent-field="model_provider_select" data-v2-agent-index="${index}" class="tcp-select">${providerSelectOptionsMarkup}</select>
        <input data-v2-agent-field="model_provider_custom" data-v2-agent-index="${index}" class="tcp-input" placeholder="Custom provider id (enabled when Custom provider selected)" value="${escapeHtml(String(seed.model_provider || ""))}" />
        <select data-v2-agent-field="model_id_select" data-v2-agent-index="${index}" class="tcp-select">
          <option value="">Default model for provider</option>
          <option value="__custom__">Custom model...</option>
        </select>
        <input data-v2-agent-field="model_id_custom" data-v2-agent-index="${index}" class="tcp-input" placeholder="Custom model id (enabled when Custom model selected)" value="${escapeHtml(String(seed.model_id || ""))}" />
        <input data-v2-agent-field="skills" data-v2-agent-index="${index}" class="tcp-input" list="automation-v2-skill-options" placeholder="Skills (text tags, comma-separated)" value="${escapeHtml(String(Array.isArray(seed.skills) ? seed.skills.join(", ") : seed.skills || ""))}" />
        <select data-v2-agent-field="tool_mode" data-v2-agent-index="${index}" class="tcp-select">
          <option value="standard">Standard tools (recommended)</option>
          <option value="read_only">Read-only tools</option>
          <option value="custom">Custom allow/deny policy</option>
        </select>
        <div class="md:col-span-2 rounded-lg border border-slate-700/60 bg-slate-900/30 p-2">
          <div class="mb-1 text-xs text-slate-400">Allowed MCP servers for this agent</div>
          <div class="grid gap-2 sm:grid-cols-2">
            ${
              connectedMcpServerNames.length
                ? connectedMcpServerNames
                    .map((server) => {
                      const checked = Array.isArray(seed.mcp_servers) && seed.mcp_servers.includes(server);
                      return `<label class="inline-flex items-center gap-2 text-xs text-slate-300">
                        <input data-v2-agent-field="mcp_server_option" data-v2-agent-index="${index}" type="checkbox" value="${escapeHtml(server)}" ${checked ? "checked" : ""} class="h-4 w-4 accent-slate-400" />
                        ${escapeHtml(server)}
                      </label>`;
                    })
                    .join("")
                : '<span class="text-xs text-slate-500">No connected MCP servers found. Connect servers in MCP tab.</span>'
            }
          </div>
        </div>
        <input data-v2-agent-field="allowlist" data-v2-agent-index="${index}" class="tcp-input md:col-span-2" list="automation-v2-tool-options" placeholder="Custom tool allowlist (comma-separated)" value="${escapeHtml(String(Array.isArray(seed.allowlist) ? seed.allowlist.join(", ") : seed.allowlist || ""))}" />
        <input data-v2-agent-field="denylist" data-v2-agent-index="${index}" class="tcp-input md:col-span-2" list="automation-v2-tool-options" placeholder="Custom tool denylist (comma-separated)" value="${escapeHtml(String(Array.isArray(seed.denylist) ? seed.denylist.join(", ") : seed.denylist || ""))}" />
      </div>
    </div>
  `;
  const v2NodeRowMarkup = (index, seed = {}) => `
    <div data-v2-node-row="${index}" class="rounded-xl border border-slate-700/60 bg-slate-900/30 p-3">
      <div class="mb-2 text-xs text-slate-300 font-semibold">Node ${index + 1}</div>
      <div class="grid gap-2 md:grid-cols-2">
        <input data-v2-node-field="node_id" data-v2-node-index="${index}" class="tcp-input font-mono text-xs" value="${escapeHtml(String(seed.node_id || `node-${index + 1}`))}" />
        <input data-v2-node-field="agent_id" data-v2-node-index="${index}" class="tcp-input font-mono text-xs" value="${escapeHtml(String(seed.agent_id || "agent-1"))}" />
        <input data-v2-node-field="objective" data-v2-node-index="${index}" class="tcp-input md:col-span-2" placeholder="Node objective" value="${escapeHtml(String(seed.objective || ""))}" />
        <input data-v2-node-field="depends_on" data-v2-node-index="${index}" class="tcp-input md:col-span-2 font-mono text-xs" placeholder="depends_on csv (node-1,node-2)" value="${escapeHtml(String(Array.isArray(seed.depends_on) ? seed.depends_on.join(", ") : seed.depends_on || ""))}" />
        <input data-v2-node-field="timeout_ms" data-v2-node-index="${index}" class="tcp-input" type="number" min="0" placeholder="timeout ms" value="${escapeHtml(String(seed.timeout_ms || ""))}" />
      </div>
    </div>
  `;
  const parseCsv = (value) =>
    String(value || "")
      .split(",")
      .map((x) => x.trim())
      .filter(Boolean);
  const syncV2AgentRowModelControls = (index) => {
    const root = byId("view");
    const readEl = (field) =>
      root.querySelector(`[data-v2-agent-index="${index}"][data-v2-agent-field="${field}"]`);
    const providerSelect = readEl("model_provider_select");
    const providerCustom = readEl("model_provider_custom");
    const modelSelect = readEl("model_id_select");
    const modelCustom = readEl("model_id_custom");
    if (!providerSelect || !providerCustom || !modelSelect || !modelCustom) return;

    const providerIsCustom = providerSelect.value === "__custom__";
    const selectedProvider = String(providerSelect.value || "").trim();
    const providerValue = providerIsCustom
      ? String(providerCustom.value || "").trim()
      : selectedProvider || initialProviderId || "";
    providerCustom.disabled = !providerIsCustom;
    providerCustom.classList.toggle("opacity-60", !providerIsCustom);
    const modelCandidates = providerValue ? modelIdsForProvider(providerValue) : [];
    const previousModel = String(modelCustom.value || "").trim();
    modelSelect.innerHTML = [
      `<option value="">Default model for provider</option>`,
      ...modelCandidates.map((id) => `<option value="${escapeHtml(id)}">${escapeHtml(id)}</option>`),
      `<option value="__custom__">Custom model...</option>`,
    ].join("");
    const currentSelect = String(modelSelect.value || "").trim();
    const hasKnownModel = !!previousModel && modelCandidates.includes(previousModel);
    if (hasKnownModel) modelSelect.value = previousModel;
    else if (previousModel || currentSelect === "__custom__") modelSelect.value = "__custom__";
    else modelSelect.value = "";
    const modelIsCustom = modelSelect.value === "__custom__";
    modelCustom.disabled = !modelIsCustom;
    modelCustom.classList.toggle("opacity-60", !modelIsCustom);
  };
  const initializeV2AgentModelControls = () => {
    const rows = [...byId("automation-v2-agents-editor").querySelectorAll("[data-v2-agent-row]")];
    for (const row of rows) {
      const index = String(row.getAttribute("data-v2-agent-index") || "").trim();
      if (!index) continue;
      const root = byId("view");
      const readEl = (field) =>
        root.querySelector(`[data-v2-agent-index="${index}"][data-v2-agent-field="${field}"]`);
      const providerSelect = readEl("model_provider_select");
      const providerCustom = readEl("model_provider_custom");
      const modelSelect = readEl("model_id_select");
      const modelCustom = readEl("model_id_custom");
      const toolMode = readEl("tool_mode");
      const allowlist = readEl("allowlist");
      const denylist = readEl("denylist");
      const seedAllow = parseCsv(String(allowlist?.value || ""));
      const seedDeny = parseCsv(String(denylist?.value || ""));
      const isReadOnlySeed = seedAllow.length === 1 && seedAllow[0] === "read" && !seedDeny.length;
      if (toolMode) {
        toolMode.value = isReadOnlySeed ? "read_only" : seedAllow.length || seedDeny.length ? "custom" : "standard";
      }
      const presetProvider = String(providerCustom?.value || "").trim();
      if (providerSelect) {
        if (presetProvider && providerIds.includes(presetProvider)) providerSelect.value = presetProvider;
        else if (presetProvider) providerSelect.value = "__custom__";
        else providerSelect.value = "";
      }
      syncV2AgentRowModelControls(index);
      if (providerSelect && providerSelect.dataset.wired !== "1") {
        providerSelect.dataset.wired = "1";
        providerSelect.addEventListener("change", () => syncV2AgentRowModelControls(index));
      }
      if (providerCustom && providerCustom.dataset.wired !== "1") {
        providerCustom.dataset.wired = "1";
        providerCustom.addEventListener("input", () => syncV2AgentRowModelControls(index));
      }
      if (modelSelect && modelSelect.dataset.wired !== "1") {
        modelSelect.dataset.wired = "1";
        modelSelect.addEventListener("change", () => syncV2AgentRowModelControls(index));
      }
      const syncToolMode = () => {
        if (!toolMode || !allowlist || !denylist) return;
        const mode = String(toolMode.value || "standard");
        const isCustom = mode === "custom";
        allowlist.disabled = !isCustom;
        denylist.disabled = !isCustom;
        allowlist.classList.toggle("opacity-60", !isCustom);
        denylist.classList.toggle("opacity-60", !isCustom);
      };
      syncToolMode();
      if (toolMode && toolMode.dataset.wired !== "1") {
        toolMode.dataset.wired = "1";
        toolMode.addEventListener("change", syncToolMode);
      }
    }
  };
  const rebuildV2AgentRows = () => {
    const count = Math.max(
      1,
      Math.min(12, Number.parseInt(String(byId("automation-v2-agent-count")?.value || "2"), 10) || 2)
    );
    byId("automation-v2-agents-editor").innerHTML = Array.from({ length: count }, (_, i) => v2AgentRowMarkup(i)).join("");
    initializeV2AgentModelControls();
  };
  const appendV2NodeRow = () => {
    const editor = byId("automation-v2-nodes-editor");
    const count = editor.querySelectorAll("[data-v2-node-row]").length;
    editor.insertAdjacentHTML("beforeend", v2NodeRowMarkup(count));
  };
  const setV2AgentsAndNodes = (agents = [], nodes = []) => {
    const safeAgents = Array.isArray(agents) && agents.length ? agents : [{ agent_id: "agent-1" }];
    byId("automation-v2-agent-count").value = String(Math.min(12, Math.max(1, safeAgents.length)));
    byId("automation-v2-agents-editor").innerHTML = safeAgents
      .slice(0, 12)
      .map((agent, i) => v2AgentRowMarkup(i, agent))
      .join("");
    initializeV2AgentModelControls();
    const safeNodes = Array.isArray(nodes) && nodes.length ? nodes : [{ node_id: "node-1", agent_id: "agent-1" }];
    byId("automation-v2-nodes-editor").innerHTML = safeNodes.map((node, i) => v2NodeRowMarkup(i, node)).join("");
  };
  const v2PresetCatalog = {
    github_bug_hunter: {
      name: "GitHub Bug Hunter",
      description: "Monitor issues, reproduce, patch, and verify fixes automatically.",
      schedule: { type: "interval", interval_seconds: 3600, timezone: "UTC", misfire_policy: "run_once" },
      agents: [
        {
          agent_id: "triage",
          display_name: "Issue Triage",
          model_provider: "openrouter",
          model_id: "openai/gpt-4o-mini",
          skills: ["issue-triage"],
          mcp_servers: ["github", "composio"],
          allowlist: ["read", "mcp.github.*", "mcp.composio.*"],
        },
        {
          agent_id: "fixer",
          display_name: "Fix Implementer",
          model_provider: "openrouter",
          model_id: "anthropic/claude-3.5-sonnet",
          skills: ["coding"],
          mcp_servers: ["github"],
          allowlist: ["read", "write", "edit", "bash", "mcp.github.*"],
        },
        {
          agent_id: "qa",
          display_name: "Regression Tester",
          model_provider: "openrouter",
          model_id: "openai/gpt-4o-mini",
          skills: ["testing"],
          mcp_servers: ["github"],
          allowlist: ["read", "bash", "mcp.github.*"],
        },
      ],
      nodes: [
        { node_id: "scan-issues", agent_id: "triage", objective: "Find high-signal open bugs and collect repro clues." },
        { node_id: "implement-fix", agent_id: "fixer", objective: "Implement minimal safe fix and prepare patch summary.", depends_on: ["scan-issues"] },
        { node_id: "run-tests", agent_id: "qa", objective: "Run targeted checks and report verification status.", depends_on: ["implement-fix"] },
      ],
    },
    code_generation_pipeline: {
      name: "Code Generation Pipeline",
      description: "Draft implementation, refine quality, and validate quickly.",
      schedule: { type: "manual", timezone: "UTC", misfire_policy: "run_once" },
      agents: [
        { agent_id: "planner", display_name: "Planner", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["read"] },
        { agent_id: "builder", display_name: "Builder", model_provider: "openrouter", model_id: "anthropic/claude-3.5-sonnet", allowlist: ["read", "write", "edit", "bash"] },
        { agent_id: "reviewer", display_name: "Reviewer", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["read", "bash"] },
      ],
      nodes: [
        { node_id: "plan", agent_id: "planner", objective: "Produce implementation plan with acceptance criteria." },
        { node_id: "implement", agent_id: "builder", objective: "Generate and refine code changes from plan.", depends_on: ["plan"] },
        { node_id: "validate", agent_id: "reviewer", objective: "Run checks/tests and summarize risks.", depends_on: ["implement"] },
      ],
    },
    release_notes_changelog: {
      name: "Release Notes + Changelog",
      description: "Collect changes and draft release comms.",
      schedule: { type: "cron", cron_expression: "0 15 * * 5", timezone: "UTC", misfire_policy: "run_once" },
      agents: [
        { agent_id: "collector", display_name: "Change Collector", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["read", "bash", "mcp.github.*"], mcp_servers: ["github"] },
        { agent_id: "writer", display_name: "Release Writer", model_provider: "openrouter", model_id: "anthropic/claude-3.5-sonnet", allowlist: ["read", "write", "edit"] },
      ],
      nodes: [
        { node_id: "collect", agent_id: "collector", objective: "Gather merged PRs/issues since last release." },
        { node_id: "draft", agent_id: "writer", objective: "Draft release notes and changelog sections.", depends_on: ["collect"] },
      ],
    },
    marketing_content_engine: {
      name: "Marketing Content Engine",
      description: "Generate campaign-ready social + email content from product updates.",
      schedule: { type: "interval", interval_seconds: 86400, timezone: "UTC", misfire_policy: "run_once" },
      agents: [
        { agent_id: "research", display_name: "Trend Research", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["websearch", "webfetch", "read"] },
        { agent_id: "copy", display_name: "Copywriter", model_provider: "openrouter", model_id: "anthropic/claude-3.5-sonnet", allowlist: ["read", "write", "edit"] },
        { agent_id: "editor", display_name: "Brand Editor", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["read", "edit"] },
      ],
      nodes: [
        { node_id: "market-scan", agent_id: "research", objective: "Find relevant trends and competitor angles." },
        { node_id: "draft-assets", agent_id: "copy", objective: "Draft LinkedIn post, email blurb, and CTA variations.", depends_on: ["market-scan"] },
        { node_id: "brand-check", agent_id: "editor", objective: "Align tone/style and produce final approved copy.", depends_on: ["draft-assets"] },
      ],
    },
    sales_lead_outreach: {
      name: "Sales Lead Outreach",
      description: "Enrich leads and generate personalized outreach drafts.",
      schedule: { type: "interval", interval_seconds: 21600, timezone: "UTC", misfire_policy: "run_once" },
      agents: [
        { agent_id: "enrichment", display_name: "Lead Enrichment", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["read", "websearch", "mcp.composio.*"], mcp_servers: ["composio"] },
        { agent_id: "outreach", display_name: "Outreach Writer", model_provider: "openrouter", model_id: "anthropic/claude-3.5-sonnet", allowlist: ["read", "write", "edit", "mcp.composio.*"], mcp_servers: ["composio"] },
      ],
      nodes: [
        { node_id: "enrich-leads", agent_id: "enrichment", objective: "Enrich lead list with role/company context." },
        { node_id: "draft-outreach", agent_id: "outreach", objective: "Create personalized outreach drafts and follow-up options.", depends_on: ["enrich-leads"] },
      ],
    },
    productivity_inbox_to_tasks: {
      name: "Inbox to Tasks",
      description: "Convert inbound messages into prioritized action items and calendar-ready summaries.",
      schedule: { type: "interval", interval_seconds: 1800, timezone: "UTC", misfire_policy: "run_once" },
      agents: [
        { agent_id: "classifier", display_name: "Inbox Classifier", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["read", "mcp.composio.*"], mcp_servers: ["composio"] },
        { agent_id: "planner", display_name: "Task Planner", model_provider: "openrouter", model_id: "openai/gpt-4o-mini", allowlist: ["read", "write", "edit", "todo_write"] },
      ],
      nodes: [
        { node_id: "classify", agent_id: "classifier", objective: "Classify inbox items into urgent, important, and informational." },
        { node_id: "task-plan", agent_id: "planner", objective: "Generate prioritized tasks with due windows and concise summaries.", depends_on: ["classify"] },
      ],
    },
  };
  rebuildV2AgentRows();
  appendV2NodeRow();
  byId("view")
    .querySelectorAll("[data-template-id]")
    .forEach((btn) =>
      btn.addEventListener("click", () => {
        const presetId = String(btn.getAttribute("data-template-id") || "").trim();
        const preset = v2PresetCatalog[presetId];
        if (!preset) {
          toast("err", "Preset not found.");
          return;
        }
        byId("automation-v2-preset").value = presetId;
        byId("automation-v2-name").value = String(preset.name || "");
        byId("automation-v2-description").value = String(preset.description || "");
        const schedule = preset.schedule || {};
        byId("automation-v2-schedule-type").value = String(schedule.type || "manual");
        byId("automation-v2-cron").value = String(schedule.cron_expression || "");
        byId("automation-v2-interval-seconds").value = String(schedule.interval_seconds || 3600);
        byId("automation-v2-timezone").value = String(schedule.timezone || "UTC");
        byId("automation-v2-misfire").value = String(schedule.misfire_policy || "run_once");
        setV2AgentsAndNodes(preset.agents || [], preset.nodes || []);
        writeAgentsUiState({ tab: "automations", wizard: true, flow: "advanced", step: 1 });
        toast("ok", `Template loaded: ${preset.name}`);
      })
    );
  byId("automation-v2-generate-agents")?.addEventListener("click", () => {
    rebuildV2AgentRows();
    toast("ok", "Agent rows regenerated.");
  });
  byId("automation-v2-add-node")?.addEventListener("click", () => {
    appendV2NodeRow();
  });
  byId("automation-v2-apply-preset")?.addEventListener("click", () => {
    const presetId = String(byId("automation-v2-preset")?.value || "").trim();
    if (!presetId) {
      toast("err", "Choose a preset first.");
      return;
    }
    const preset = v2PresetCatalog[presetId];
    if (!preset) {
      toast("err", "Preset not found.");
      return;
    }
    byId("automation-v2-name").value = String(preset.name || "");
    byId("automation-v2-description").value = String(preset.description || "");
    const schedule = preset.schedule || {};
    byId("automation-v2-schedule-type").value = String(schedule.type || "manual");
    byId("automation-v2-cron").value = String(schedule.cron_expression || "");
    byId("automation-v2-interval-seconds").value = String(schedule.interval_seconds || 3600);
    byId("automation-v2-timezone").value = String(schedule.timezone || "UTC");
    byId("automation-v2-misfire").value = String(schedule.misfire_policy || "run_once");
    setV2AgentsAndNodes(preset.agents || [], preset.nodes || []);
    toast("ok", `Preset applied: ${preset.name}`);
  });
  byId("automation-v2-create")?.addEventListener("click", async () => {
    try {
      const name = String(byId("automation-v2-name")?.value || "").trim();
      if (!name) throw new Error("Automation name is required.");
      const description = String(byId("automation-v2-description")?.value || "").trim();
      const scheduleType = String(byId("automation-v2-schedule-type")?.value || "manual").trim();
      const timezone = String(byId("automation-v2-timezone")?.value || "UTC").trim() || "UTC";
      const misfire = String(byId("automation-v2-misfire")?.value || "run_once").trim();
      const cronExpression = String(byId("automation-v2-cron")?.value || "").trim();
      const intervalSeconds = Number.parseInt(
        String(byId("automation-v2-interval-seconds")?.value || "3600"),
        10
      );
      if (scheduleType === "cron" && !cronExpression) {
        throw new Error("Cron expression is required for cron schedule.");
      }
      if (scheduleType === "interval" && !(Number.isFinite(intervalSeconds) && intervalSeconds > 0)) {
        throw new Error("Interval seconds must be greater than 0.");
      }

      const agentRows = [...byId("automation-v2-agents-editor").querySelectorAll("[data-v2-agent-row]")];
      const seenAgents = new Set();
      const agents = [];
      for (const row of agentRows) {
        const idx = String(row.getAttribute("data-v2-agent-index") || "");
        const read = (field) =>
          byId("view").querySelector(`[data-v2-agent-index="${idx}"][data-v2-agent-field="${field}"]`)?.value || "";
        const agentId = String(read("agent_id")).trim();
        if (!agentId) continue;
        if (seenAgents.has(agentId)) throw new Error(`Duplicate agent_id: ${agentId}`);
        seenAgents.add(agentId);
        const providerSelect = String(read("model_provider_select")).trim();
        const providerCustom = String(read("model_provider_custom")).trim();
        const modelSelect = String(read("model_id_select")).trim();
        const modelCustom = String(read("model_id_custom")).trim();
        const modelProvider =
          providerSelect === "__custom__"
            ? providerCustom
            : providerSelect || String(read("model_provider")).trim();
        const modelId =
          modelSelect === "__custom__" ? modelCustom : modelSelect || String(read("model_id")).trim();
        const toolMode = String(read("tool_mode")).trim() || "standard";
        const selectedMcpServers = [
          ...byId("view").querySelectorAll(
            `[data-v2-agent-index="${idx}"][data-v2-agent-field="mcp_server_option"]:checked`
          ),
        ]
          .map((node) => String(node.value || "").trim())
          .filter(Boolean);
        const allowlist =
          toolMode === "custom"
            ? parseCsv(read("allowlist"))
            : toolMode === "read_only"
              ? ["read"]
              : [];
        const denylist = toolMode === "custom" ? parseCsv(read("denylist")) : [];
        agents.push({
          agent_id: agentId,
          display_name: String(read("display_name")).trim() || agentId,
          model_policy:
            modelProvider && modelId
              ? { default_model: { provider_id: modelProvider, model_id: modelId } }
              : undefined,
          skills: parseCsv(read("skills")),
          tool_policy: {
            allowlist,
            denylist,
          },
          mcp_policy: {
            allowed_servers: selectedMcpServers,
          },
        });
      }
      if (!agents.length) throw new Error("At least one agent is required.");

      const nodeRows = [...byId("automation-v2-nodes-editor").querySelectorAll("[data-v2-node-row]")];
      const seenNodes = new Set();
      const nodes = [];
      for (const row of nodeRows) {
        const idx = String(row.getAttribute("data-v2-node-index") || "");
        const read = (field) =>
          byId("view").querySelector(`[data-v2-node-index="${idx}"][data-v2-node-field="${field}"]`)?.value || "";
        const nodeId = String(read("node_id")).trim();
        const objective = String(read("objective")).trim();
        const agentId = String(read("agent_id")).trim();
        if (!nodeId || !objective || !agentId) continue;
        if (seenNodes.has(nodeId)) throw new Error(`Duplicate node_id: ${nodeId}`);
        seenNodes.add(nodeId);
        const timeoutMs = Number.parseInt(String(read("timeout_ms")).trim(), 10);
        nodes.push({
          node_id: nodeId,
          objective,
          agent_id: agentId,
          depends_on: parseCsv(read("depends_on")),
          timeout_ms: Number.isFinite(timeoutMs) && timeoutMs > 0 ? timeoutMs : undefined,
        });
      }
      if (!nodes.length) throw new Error("At least one flow node is required.");

      const payload = {
        name,
        description: description || undefined,
        status: "active",
        schedule: {
          type: scheduleType,
          cron_expression: scheduleType === "cron" ? cronExpression : undefined,
          interval_seconds: scheduleType === "interval" ? intervalSeconds : undefined,
          timezone,
          misfire_policy: misfire,
        },
        agents,
        flow: { nodes },
        execution: { max_parallel_agents: Math.min(agents.length, 4) },
      };
      await v2Request((client) => client.create(payload));
      toast("ok", "Automation created.");
      renderAgents(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });
  const runReview = async (runId, family, decision) => {
    const isAutomation = String(family || "").toLowerCase() === "automation";
    const action = String(decision || "").toLowerCase() === "deny" ? "deny" : "approve";
    if (!runId) throw new Error("Run ID is missing.");
    if (action === "approve") {
      if (isAutomation) {
        await state.client.automations.approveRun(runId, "approved from control panel");
      } else {
        await state.client.routines.approveRun(runId, "approved from control panel");
      }
      return;
    }
    if (isAutomation) {
      await state.client.automations.denyRun(runId, "denied from control panel");
    } else {
      await state.client.routines.denyRun(runId, "denied from control panel");
    }
  };
  const wireRunReviewButtons = (rootEl) => {
    rootEl.querySelectorAll("[data-run-review]").forEach((b) => {
      if (b.dataset.wired === "1") return;
      b.dataset.wired = "1";
      b.addEventListener("click", async () => {
        const runId = String(b.dataset.runId || "").trim();
        const family = String(b.dataset.runFamily || "routine").trim().toLowerCase();
        const decision = String(b.dataset.runReview || "approve").trim().toLowerCase();
        if (!runId) {
          toast("err", "Run ID is missing.");
          return;
        }
        const peerButtons = [...byId("view").querySelectorAll("[data-run-review]")].filter(
          (node) =>
            String(node.dataset.runId || "").trim() === runId &&
            String(node.dataset.runFamily || "routine").trim().toLowerCase() === family
        );
        const original = new Map();
        for (const node of peerButtons) {
          original.set(node, node.innerHTML);
          node.disabled = true;
          node.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i>';
          renderIcons(node);
        }
        try {
          await runReview(runId, family, decision);
          toast("ok", `${decision === "deny" ? "Denied" : "Approved"} ${family} run ${runId}.`);
          setTimeout(() => {
            renderAgents(ctx);
          }, 250);
        } catch (e) {
          toast("err", e instanceof Error ? e.message : String(e));
          for (const node of peerButtons) {
            if (!node.isConnected) continue;
            node.disabled = false;
            node.innerHTML = original.get(node) || node.innerHTML;
            renderIcons(node);
          }
        }
      });
    });
  };
  wireRunReviewButtons(byId("view"));
  byId("view").querySelectorAll("[data-inspect-run]").forEach((b) =>
    b.addEventListener("click", async () => {
      const runId = String(b.dataset.inspectRun || "").trim();
      const family = String(b.dataset.runFamily || "routine").trim();
      if (!runId) {
        toast("err", "Run ID is missing.");
        return;
      }
      const prev = b.innerHTML;
      b.disabled = true;
      b.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i> Loading';
      renderIcons(b);
      try {
        let run = await state.client.routines.getRun(runId).catch(() => null);
        let artifactsPayload = await state.client.routines.listArtifacts(runId).catch(() => null);
        if (!run && family === "automation") {
          run = await state.client.automations.getRun(runId).catch(() => null);
          artifactsPayload = await state.client.automations.listArtifacts(runId).catch(() => null);
        }
        if (!run) throw new Error("Run details not found.");
        const artifacts = Array.isArray(artifactsPayload?.artifacts) ? artifactsPayload.artifacts : [];
        const runAllowedTools = listFromRoutine(run, "allowed_tools", "allowedTools");
        const runStatus = runStatusOf(run);
        const runRequiresApproval = boolFromRoutine(
          run,
          "requires_approval",
          "requiresApproval",
          true
        );
        const runExternalAllowed = boolFromRoutine(
          run,
          "external_integrations_allowed",
          "externalIntegrationsAllowed",
          false
        );
        runInspectorEl.innerHTML = `
          <div class="tcp-list-item">
            <div class="mb-2 flex flex-wrap items-center gap-2">
              <span class="${runStatusClass(runStatus)}">${escapeHtml(runStatus)}</span>
              <span class="tcp-subtle font-mono">${escapeHtml(runId)}</span>
              <span class="tcp-subtle">${escapeHtml(formatTimestamp(firstTimestamp(run)))}</span>
            </div>
            <div class="mb-2 text-xs text-slate-300">
              ${escapeHtml(
                runAllowedTools.length
                  ? `Allowlist (${runAllowedTools.length}): ${runAllowedTools.join(", ")}`
                  : "No explicit allowlist: all tools are available subject to policy."
              )}
            </div>
            <div class="mb-2 flex flex-wrap items-center gap-2 text-xs">
              <span class="${runRequiresApproval ? "tcp-badge-warn" : "tcp-badge-info"}">${runRequiresApproval ? "approval required" : "no approval gate"}</span>
              <span class="${runExternalAllowed ? "tcp-badge-info" : "tcp-badge-warn"}">${runExternalAllowed ? "external integrations allowed" : "external integrations blocked"}</span>
            </div>
            ${
              isPendingApprovalStatus(runStatus)
                ? `<div class="mb-2 flex flex-wrap items-center gap-2 text-xs">
                <button data-run-review="approve" data-run-id="${escapeHtml(runId)}" data-run-family="${escapeHtml(family)}" class="tcp-btn h-7 px-2 text-xs">Approve</button>
                <button data-run-review="deny" data-run-id="${escapeHtml(runId)}" data-run-family="${escapeHtml(family)}" class="tcp-btn-danger h-7 px-2 text-xs">Deny</button>
              </div>`
                : ""
            }
            <div class="mb-2 text-xs text-slate-300">${escapeHtml(runDetailOf(run) || "No detail text available.")}</div>
            <div class="mb-2 text-xs text-slate-400">Artifacts: ${artifacts.length}</div>
            ${
              artifacts.length
                ? `<div class="mb-2 grid gap-1">${artifacts
                    .map((a) => {
                      const uri = String(a?.uri || "").trim();
                      const kind = String(a?.kind || "artifact");
                      const label = String(a?.label || kind).trim();
                      return `<div class="text-xs text-slate-300"><span class="tcp-subtle">${escapeHtml(kind)}</span> <span class="font-mono">${escapeHtml(label)}</span> ${uri ? `<span class="tcp-subtle">${escapeHtml(uri)}</span>` : ""}</div>`;
                    })
                    .join("")}</div>`
                : ""
            }
            <details class="mt-2">
              <summary class="cursor-pointer text-xs text-slate-400">Raw run payload</summary>
              <pre class="tcp-code mt-2">${escapeHtml(JSON.stringify(run, null, 2))}</pre>
            </details>
          </div>
        `;
        wireRunReviewButtons(runInspectorEl);
        renderIcons(runInspectorEl);
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
  byId("view").querySelectorAll("[data-v2-run-now]").forEach((b) =>
    b.addEventListener("click", async () => {
      const automationId = String(b.dataset.v2RunNow || "").trim();
      if (!automationId) return;
      const prev = b.innerHTML;
      b.disabled = true;
      b.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i> Running...';
      renderIcons(b);
      try {
        const response = await v2Request((client) => client.runNow(automationId));
        const runId = String(response?.run?.run_id || response?.run?.runId || "").trim();
        toast("ok", runId ? `Automation triggered (run ${runId}).` : "Automation triggered.");
        setTimeout(() => renderAgents(ctx), 450);
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
  byId("view").querySelectorAll("[data-v2-toggle]").forEach((b) =>
    b.addEventListener("click", async () => {
      const automationId = String(b.dataset.v2Toggle || "").trim();
      const next = String(b.dataset.v2Next || "").trim().toLowerCase();
      if (!automationId) return;
      const prev = b.innerHTML;
      b.disabled = true;
      b.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i> Saving...';
      renderIcons(b);
      try {
        if (next === "pause") {
          await v2Request((client) => client.pause(automationId, "paused from control panel"));
          toast("ok", "Automation paused.");
        } else {
          await v2Request((client) => client.resume(automationId));
          toast("ok", "Automation resumed.");
        }
        setTimeout(() => renderAgents(ctx), 300);
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
  byId("view").querySelectorAll("[data-v2-runs]").forEach((b) =>
    b.addEventListener("click", async () => {
      const automationId = String(b.dataset.v2Runs || "").trim();
      if (!automationId) return;
      const prev = b.innerHTML;
      b.disabled = true;
      b.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i> Loading';
      renderIcons(b);
      try {
        const payload = await v2Request((client) => client.listRuns(automationId, 20));
        const runs = Array.isArray(payload?.runs) ? payload.runs : [];
        const runRows =
          runs
            .map((run) => {
              const runId = String(run?.run_id || run?.runId || "").trim();
              const status = String(run?.status || "unknown").toLowerCase();
              const updatedAt = Number(run?.updated_at_ms || run?.updatedAtMs || 0);
              return `<div class="rounded-lg border border-slate-700/60 p-2">
                <div class="flex items-center justify-between gap-2">
                  <span class="font-mono text-xs">${escapeHtml(runId || "run n/a")}</span>
                  <span class="${runStatusClass(status)}">${escapeHtml(status)}</span>
                </div>
                <div class="mt-1 flex flex-wrap items-center gap-2 text-xs">
                  <span class="tcp-subtle">${escapeHtml(formatTimestamp(updatedAt))}</span>
                  ${
                    runId
                      ? `<button data-v2-run-action="pause" data-v2-run-id="${escapeHtml(runId)}" class="tcp-btn h-7 px-2 text-xs">Pause</button>
                         <button data-v2-run-action="resume" data-v2-run-id="${escapeHtml(runId)}" class="tcp-btn h-7 px-2 text-xs">Resume</button>
                         <button data-v2-run-action="cancel" data-v2-run-id="${escapeHtml(runId)}" class="tcp-btn-danger h-7 px-2 text-xs">Cancel</button>`
                      : ""
                  }
                </div>
                <details class="mt-1">
                  <summary class="cursor-pointer text-xs text-slate-400">Run payload</summary>
                  <pre class="tcp-code mt-1">${escapeHtml(JSON.stringify(run, null, 2))}</pre>
                </details>
              </div>`;
            })
            .join("") || '<p class="tcp-subtle">No runs found for this automation.</p>';
        v2RunInspectorEl.innerHTML = `<div class="tcp-list-item">
          <div class="mb-2 flex items-center justify-between gap-2">
            <span class="font-medium">Automation: ${escapeHtml(automationId)}</span>
            <span class="tcp-subtle">${runs.length} runs</span>
          </div>
          <div class="grid gap-2">${runRows}</div>
        </div>`;
        v2RunInspectorEl.querySelectorAll("[data-v2-run-action]").forEach((btn) =>
          btn.addEventListener("click", async () => {
            const action = String(btn.dataset.v2RunAction || "").trim();
            const runId = String(btn.dataset.v2RunId || "").trim();
            if (!runId) return;
            const innerPrev = btn.innerHTML;
            btn.disabled = true;
            btn.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i>';
            renderIcons(btn);
            try {
              if (action === "pause") {
                await v2Request((client) => client.pauseRun(runId, "paused from control panel"));
              } else if (action === "resume") {
                await v2Request((client) => client.resumeRun(runId, "resumed from control panel"));
              } else {
                await v2Request((client) => client.cancelRun(runId, "cancelled from control panel"));
              }
              toast("ok", `Run ${runId} ${action} requested.`);
              setTimeout(() => renderAgents(ctx), 300);
            } catch (e) {
              toast("err", e instanceof Error ? e.message : String(e));
            } finally {
              if (btn.isConnected) {
                btn.disabled = false;
                btn.innerHTML = innerPrev;
                renderIcons(btn);
              }
            }
          })
        );
        renderIcons(v2RunInspectorEl);
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

  routineList.querySelectorAll("[data-toggle-status]").forEach((b) =>
    b.addEventListener("click", async () => {
      const routineId = String(b.dataset.toggleStatus || "").trim();
      const nextStatus = String(b.dataset.nextStatus || "").trim();
      if (!routineId || !nextStatus) {
        toast("err", "Routine status action is missing details. Refresh and try again.");
        return;
      }
      const prev = b.innerHTML;
      b.disabled = true;
      b.innerHTML = '<i data-lucide="refresh-cw" class="animate-spin"></i> Saving...';
      renderIcons(b);
      try {
        await state.client.routines.update(routineId, { status: nextStatus });
        toast("ok", `Routine ${nextStatus === "paused" ? "paused" : "resumed"}.`);
        renderAgents(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
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
  routineList.querySelectorAll("[data-edit-routine]").forEach((b) =>
    b.addEventListener("click", async () => {
      const routineId = String(b.dataset.editRoutine || "").trim();
      if (!routineId) {
        toast("err", "Routine ID is missing.");
        return;
      }
      const routine = routineById.get(routineId);
      if (!routine) {
        toast("err", "Routine details not found. Refresh and try again.");
        return;
      }
      nameEl.value = String(routine.name || "").trim();
      applyScheduleToForm(routine.schedule);
      const promptFilePath = detectPromptFile(routine);
      if (promptFilePath) {
        useFileEl.checked = true;
        pathEl.value = promptFilePath;
        try {
          const payload = await api(`/api/files/read?path=${encodeURIComponent(promptFilePath)}`);
          promptEl.value = String(payload?.text || "");
        } catch {
          promptEl.value = String(routine.entrypoint || "");
          toast("err", `Could not read ${promptFilePath}; loaded entrypoint text instead.`);
        }
      } else {
        useFileEl.checked = false;
        promptEl.value = String(routine.entrypoint || routine.prompt || "");
        normalizeFilePath();
      }
      allowedToolsEl.value = listFromRoutine(routine, "allowed_tools", "allowedTools").join(", ");
      requiresApprovalEl.checked = boolFromRoutine(
        routine,
        "requires_approval",
        "requiresApproval",
        true
      );
      externalIntegrationsEl.checked = boolFromRoutine(
        routine,
        "external_integrations_allowed",
        "externalIntegrationsAllowed",
        false
      );
      const routineAllowEverything = isAllowEverythingPolicy(
        normalizeToolList(allowedToolsEl.value || ""),
        !!requiresApprovalEl.checked,
        !!externalIntegrationsEl.checked
      );
      setAllowEverythingState(routineAllowEverything, { restore: false });
      renderToolScopePreview();
      setRoutineModelFromSpec(routine);
      setEditMode(routineId, String(routine.name || ""));
      nameEl.focus();
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
  const modelProviderEl = byId("routine-model-provider");
  const modelIdEl = byId("routine-model-id");
  const modelPreviewEl = byId("routine-model-preview");
  const allowEverythingEl = byId("routine-allow-everything");
  const allowedToolsEl = byId("routine-allowed-tools");
  const toolScopePreviewEl = byId("routine-tool-scope-preview");
  const requiresApprovalEl = byId("routine-requires-approval");
  const externalIntegrationsEl = byId("routine-external-integrations");
  const routineFormModeEl = byId("routine-form-mode");
  const cancelEditEl = byId("cancel-edit-routine");
  const createRoutineEl = byId("create-routine");
  let editingRoutineId = "";

  const normalizeFilePath = () => {
    const fallback = `control-panel/routines/${slugify(nameEl.value || "new-routine")}.md`;
    const raw = String(pathEl.value || "").trim().replace(/\\/g, "/").replace(/^\/+/, "");
    const next = raw || fallback;
    const prefixed =
      next === "control-panel" || next.startsWith("control-panel/") ? next : `control-panel/${next}`;
    pathEl.value = prefixed.endsWith(".md") ? prefixed : `${prefixed}.md`;
  };

  const preferredModelForProvider = (providerId) => {
    const models = modelIdsForProvider(providerId);
    if (!models.length) return "";
    const configured = String(providerDefaults[providerId] || "").trim();
    if (configured && models.includes(configured)) return configured;
    return models[0];
  };

  const renderModelPicker = () => {
    const providerId = String(modelProviderEl?.value || "").trim();
    const models = modelIdsForProvider(providerId);
    if (!modelIdEl) return;
    modelIdEl.innerHTML =
      models.map((modelId) => `<option value="${escapeHtml(modelId)}">${escapeHtml(modelId)}</option>`).join("") ||
      '<option value="">No models found</option>';
    if (!models.length) {
      modelIdEl.disabled = true;
      if (modelPreviewEl) modelPreviewEl.textContent = "default engine route";
      return;
    }
    modelIdEl.disabled = false;
    const preferred = preferredModelForProvider(providerId);
    if (preferred) modelIdEl.value = preferred;
    const modelId = String(modelIdEl.value || "").trim();
    if (modelPreviewEl) modelPreviewEl.textContent = providerId && modelId ? `${providerId}/${modelId}` : "default engine route";
  };

  const renderToolScopePreview = () => {
    const allowlist = normalizeToolList(allowedToolsEl?.value || "");
    if (!toolScopePreviewEl) return;
    if (allowEverythingEl?.checked) {
      toolScopePreviewEl.textContent = "Tool scope: unrestricted (all tools + external integrations, no approval gate)";
      return;
    }
    toolScopePreviewEl.textContent = allowlist.length
      ? `Tool scope: allowlist (${allowlist.length})`
      : "Tool scope: all tools allowed by policy";
  };
  const isAllowEverythingPolicy = (allowedTools, requiresApproval, externalIntegrationsAllowed) =>
    !requiresApproval && externalIntegrationsAllowed && allowedTools.length === 0;
  const setAllowEverythingState = (enabled, { restore = true } = {}) => {
    if (!allowEverythingEl) return;
    allowEverythingEl.checked = !!enabled;
    if (enabled) {
      allowEverythingEl.dataset.prevTools = String(allowedToolsEl?.value || "");
      allowEverythingEl.dataset.prevRequiresApproval = requiresApprovalEl?.checked ? "1" : "0";
      allowEverythingEl.dataset.prevExternalIntegrations = externalIntegrationsEl?.checked ? "1" : "0";
      if (allowedToolsEl) {
        allowedToolsEl.value = "";
        allowedToolsEl.disabled = true;
      }
      if (requiresApprovalEl) {
        requiresApprovalEl.checked = false;
        requiresApprovalEl.disabled = true;
      }
      if (externalIntegrationsEl) {
        externalIntegrationsEl.checked = true;
        externalIntegrationsEl.disabled = true;
      }
    } else {
      if (allowedToolsEl) allowedToolsEl.disabled = false;
      if (requiresApprovalEl) requiresApprovalEl.disabled = false;
      if (externalIntegrationsEl) externalIntegrationsEl.disabled = false;
      if (restore) {
        if (allowedToolsEl) {
          allowedToolsEl.value = String(allowEverythingEl.dataset.prevTools || "");
        }
        if (requiresApprovalEl) {
          requiresApprovalEl.checked = String(allowEverythingEl.dataset.prevRequiresApproval || "1") === "1";
        }
        if (externalIntegrationsEl) {
          externalIntegrationsEl.checked =
            String(allowEverythingEl.dataset.prevExternalIntegrations || "0") === "1";
        }
      }
    }
    renderToolScopePreview();
  };

  const applyScheduleToForm = (schedule) => {
    const intervalSeconds = Number(
      schedule?.interval_seconds?.seconds ??
        schedule?.intervalSeconds?.seconds ??
        schedule?.intervalSeconds ??
        0
    );
    const cronExpr = String(
      schedule?.cron?.expression ??
        schedule?.cron?.cron ??
        schedule?.expression ??
        schedule?.cron ??
        ""
    ).trim();
    if (intervalSeconds > 0) {
      if (intervalSeconds % 3600 === 0) {
        scheduleModeEl.value = "interval";
        intervalUnitEl.value = "hours";
        intervalValueEl.value = String(Math.max(1, Math.floor(intervalSeconds / 3600)));
      } else {
        scheduleModeEl.value = "interval";
        intervalUnitEl.value = "minutes";
        intervalValueEl.value = String(Math.max(1, Math.floor(intervalSeconds / 60)));
      }
      renderScheduleInputs();
      return;
    }
    if (!cronExpr) {
      scheduleModeEl.value = "manual";
      renderScheduleInputs();
      return;
    }
    const daily = cronExpr.match(/^(\d{1,2})\s+(\d{1,2})\s+\*\s+\*\s+\*$/);
    if (daily) {
      scheduleModeEl.value = "daily";
      const mm = String(Number.parseInt(daily[1], 10)).padStart(2, "0");
      const hh = String(Number.parseInt(daily[2], 10)).padStart(2, "0");
      timeEl.value = `${hh}:${mm}`;
      renderScheduleInputs();
      return;
    }
    const weekly = cronExpr.match(/^(\d{1,2})\s+(\d{1,2})\s+\*\s+\*\s+([0-6])$/);
    if (weekly) {
      scheduleModeEl.value = "weekly";
      const mm = String(Number.parseInt(weekly[1], 10)).padStart(2, "0");
      const hh = String(Number.parseInt(weekly[2], 10)).padStart(2, "0");
      weekdayEl.value = String(Number.parseInt(weekly[3], 10));
      weeklyTimeEl.value = `${hh}:${mm}`;
      renderScheduleInputs();
      return;
    }
    scheduleModeEl.value = "customCron";
    cronEl.value = cronExpr;
    renderScheduleInputs();
  };

  const setRoutineModelFromSpec = (routine) => {
    const providerId = String(routine?.args?.model_policy?.default_model?.provider_id || "").trim();
    const modelId = String(routine?.args?.model_policy?.default_model?.model_id || "").trim();
    if (!providerId || !providerIds.includes(providerId)) {
      renderModelPicker();
      return;
    }
    modelProviderEl.value = providerId;
    renderModelPicker();
    const models = modelIdsForProvider(providerId);
    if (modelId && models.includes(modelId)) {
      modelIdEl.value = modelId;
      if (modelPreviewEl) modelPreviewEl.textContent = `${providerId}/${modelId}`;
    }
  };

  const setEditMode = (routineId, routineName = "") => {
    editingRoutineId = routineId;
    if (routineFormModeEl) {
      routineFormModeEl.textContent = `Editing routine ${routineName || routineId}`;
    }
    if (createRoutineEl) {
      createRoutineEl.innerHTML = '<i data-lucide="save"></i> Save Changes';
    }
    if (cancelEditEl) {
      cancelEditEl.classList.remove("hidden");
    }
    renderIcons(byId("view"));
  };

  const clearEditMode = () => {
    editingRoutineId = "";
    if (routineFormModeEl) routineFormModeEl.textContent = "Creating new routine";
    if (createRoutineEl) createRoutineEl.innerHTML = '<i data-lucide="plus"></i> Create';
    if (cancelEditEl) cancelEditEl.classList.add("hidden");
    renderIcons(byId("view"));
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
  modelProviderEl?.addEventListener("change", () => {
    renderModelPicker();
  });
  modelIdEl?.addEventListener("change", () => {
    const providerId = String(modelProviderEl?.value || "").trim();
    const modelId = String(modelIdEl?.value || "").trim();
    if (modelPreviewEl) modelPreviewEl.textContent = providerId && modelId ? `${providerId}/${modelId}` : "default engine route";
  });
  allowEverythingEl?.addEventListener("change", () => {
    setAllowEverythingState(!!allowEverythingEl.checked, { restore: true });
  });
  allowedToolsEl?.addEventListener("input", renderToolScopePreview);
  cancelEditEl?.addEventListener("click", () => {
    renderAgents(ctx);
  });
  renderScheduleInputs();
  normalizeFilePath();
  renderModelPicker();
  setAllowEverythingState(!!allowEverythingEl?.checked, { restore: false });
  renderToolScopePreview();
  clearEditMode();

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
      const args = {};
      if (promptFilePath) args.promptFilePath = promptFilePath;
      const selectedProviderId = String(modelProviderEl?.value || "").trim();
      const selectedModelId = String(modelIdEl?.value || "").trim();
      if (selectedProviderId && selectedModelId) {
        args.model_policy = {
          default_model: {
            provider_id: selectedProviderId,
            model_id: selectedModelId,
          },
        };
      }
      const allowEverything = !!allowEverythingEl?.checked;
      const allowedTools = allowEverything ? [] : normalizeToolList(allowedToolsEl?.value || "");
      const requiresApproval = allowEverything ? false : !!requiresApprovalEl?.checked;
      const externalIntegrationsAllowed = allowEverything ? true : !!externalIntegrationsEl?.checked;
      if (editingRoutineId) {
        const current = routineById.get(editingRoutineId);
        const patch = {
          name,
          entrypoint,
          schedule,
          args,
          allowed_tools: allowedTools,
          requires_approval: requiresApproval,
          external_integrations_allowed: externalIntegrationsAllowed,
        };
        if (manualOnly) {
          patch.status = "paused";
        } else if (current?.status) {
          patch.status = String(current.status).toLowerCase();
        }
        await state.client.routines.update(editingRoutineId, patch);
        toast("ok", "Routine updated.");
      } else {
        const created = await state.client.routines.create({
          name,
          entrypoint,
          schedule,
          args,
          allowed_tools: allowedTools,
          requires_approval: requiresApproval,
          external_integrations_allowed: externalIntegrationsAllowed,
        });
        if (manualOnly) {
          const routineId = routineKey(created?.routine || created || {});
          if (routineId) {
            await state.client.routines.update(routineId, { status: "paused" });
          }
        }
        toast("ok", "Routine created.");
      }
      renderAgents(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });
}
