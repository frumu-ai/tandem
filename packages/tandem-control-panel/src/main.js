import { TandemClient } from "@frumu/tandem-client";
import { animate, stagger } from "motion";
import "./styles.css";
import { api } from "./app/api.js";
import { byId, escapeHtml } from "./app/dom.js";
import { routeFromHash, ensureRoute, setHashRoute } from "./app/router.js";
import { createToasts } from "./app/toasts.js";
import { createState, NAV_ROUTES, ROUTES, providerHints } from "./app/store.js";
import { VIEW_RENDERERS } from "./views/index.js";
import { renderIcons } from "./app/icons.js";
import { THEMES, applyTheme, getActiveThemeId, setControlPanelTheme } from "./app/themes.js";

const app = document.getElementById("app");
const state = createState();
const { toast, renderToasts } = createToasts(state);
const TOKEN_STORAGE_KEY = "tandem_control_panel_token";
let routeRenderSeq = 0;

const ctx = {
  app,
  state,
  api,
  byId,
  escapeHtml,
  ROUTES,
  providerHints,
  toast,
  addCleanup,
  clearCleanup,
  setRoute,
  renderShell,
  refreshProviderStatus,
  refreshIdentityStatus,
  renderIcons,
  THEMES,
  setTheme,
};

function setTheme(themeId) {
  const theme = setControlPanelTheme(themeId);
  state.themeId = theme.id;
  return theme;
}

function addCleanup(fn) {
  state.cleanup.push(fn);
}

function clearCleanup() {
  for (const fn of state.cleanup) {
    try {
      fn();
    } catch {
      // ignore cleanup failure
    }
  }
  state.cleanup = [];
}

function getSavedToken() {
  try {
    return localStorage.getItem(TOKEN_STORAGE_KEY) || "";
  } catch {
    return "";
  }
}

function saveToken(token) {
  try {
    localStorage.setItem(TOKEN_STORAGE_KEY, token);
  } catch {
    // ignore storage failures
  }
}

function clearSavedToken() {
  try {
    localStorage.removeItem(TOKEN_STORAGE_KEY);
  } catch {
    // ignore storage failures
  }
}

async function checkAuth() {
  try {
    const me = await api("/api/auth/me", { method: "GET" });
    state.authed = true;
    state.me = me;
    state.client = new TandemClient({ baseUrl: "/api/engine", token: "session" });
    await Promise.all([refreshProviderStatus(), refreshIdentityStatus()]);
  } catch {
    state.authed = false;
    state.me = null;
    state.client = null;
    state.needsProviderOnboarding = false;
    state.providerReady = false;
    state.providerDefault = "";
    state.providerConnected = [];
    state.providerError = "";
    state.botName = "Tandem";
    state.botAvatarUrl = "";
    state.controlPanelName = "Tandem Control Panel";
    state.themeId = getActiveThemeId();
  }
}

async function refreshProviderStatus() {
  if (!state.client) {
    state.needsProviderOnboarding = false;
    state.providerReady = false;
    state.providerDefault = "";
    state.providerDefaultModel = "";
    state.providerConnected = [];
    state.providerError = "";
    return;
  }
  try {
    const [config, catalog] = await Promise.all([
      state.client.providers.config(),
      state.client.providers.catalog(),
    ]);
    const defaultProvider = String(config?.default || "").trim();
    const defaultModel = String(config?.providers?.[defaultProvider]?.default_model || "").trim();
    const connected = new Set(catalog?.connected || []);
    const ready =
      !!defaultProvider && (defaultProvider === "ollama" || connected.has(defaultProvider));
    state.providerDefault = defaultProvider;
    state.providerDefaultModel = defaultModel;
    state.providerConnected = [...connected];
    state.providerReady = ready;
    state.providerError = "";
    state.needsProviderOnboarding = !ready;
  } catch (e) {
    state.providerReady = false;
    state.providerDefault = "";
    state.providerDefaultModel = "";
    state.providerConnected = [];
    state.providerError = e instanceof Error ? e.message : String(e);
    state.needsProviderOnboarding = true;
  }
}

async function refreshIdentityStatus() {
  if (!state.client) {
    state.botName = "Tandem";
    state.botAvatarUrl = "";
    state.controlPanelName = "Tandem Control Panel";
    return;
  }
  try {
    const payload = state.client?.identity?.get
      ? await state.client.identity.get()
      : await api("/api/engine/config/identity", { method: "GET" });
    const identity = payload?.identity || {};
    const canonical = String(
      identity?.bot?.canonical_name || identity?.bot?.canonicalName || ""
    ).trim();
    const aliases = identity?.bot?.aliases || {};
    const avatar = String(identity?.bot?.avatar_url || identity?.bot?.avatarUrl || "").trim();
    const controlPanelAlias = String(aliases?.control_panel || aliases?.controlPanel || "").trim();
    state.botName = canonical || "Tandem";
    state.botAvatarUrl = avatar;
    state.controlPanelName = controlPanelAlias || `${state.botName} Control Panel`;
  } catch {
    state.botName = "Tandem";
    state.botAvatarUrl = "";
    state.controlPanelName = "Tandem Control Panel";
  }
}

function setRoute(route) {
  const nextRoute = ensureRoute(route, ROUTES);
  const targetHash = `#/${nextRoute}`;
  if (window.location.hash !== targetHash) {
    setHashRoute(nextRoute);
    return;
  }
  state.route = nextRoute;
  if (!state.authed) {
    renderLogin();
    return;
  }
  clearCleanup();
  void renderRoute({ showLoading: false });
}

function renderLogin() {
  const savedToken = getSavedToken();
  app.innerHTML = `
    <main class="mx-auto grid min-h-screen w-full max-w-3xl place-items-center p-5">
      <section class="tcp-panel w-full max-w-xl">
        <div class="mb-6 rounded-2xl border border-slate-700 bg-black/20 p-3">
          <svg viewBox="0 0 520 160" class="hero-svg chip-hero" aria-hidden="true">
            <defs>
              <linearGradient id="hero-trace-grad" x1="0" y1="0" x2="1" y2="0">
                <stop offset="0%" stop-color="#64748b" stop-opacity="0.22"></stop>
                <stop offset="50%" stop-color="#cbd5e1" stop-opacity="0.92"></stop>
                <stop offset="100%" stop-color="#64748b" stop-opacity="0.22"></stop>
              </linearGradient>
              <radialGradient id="hero-chip-core" cx="50%" cy="50%" r="60%">
                <stop offset="0%" stop-color="#f8fafc" stop-opacity="0.92"></stop>
                <stop offset="100%" stop-color="#64748b" stop-opacity="0.18"></stop>
              </radialGradient>
              <filter id="hero-chip-glow" x="-50%" y="-50%" width="200%" height="200%">
                <feGaussianBlur stdDeviation="3.5" result="blur"></feGaussianBlur>
                <feMerge>
                  <feMergeNode in="blur"></feMergeNode>
                  <feMergeNode in="SourceGraphic"></feMergeNode>
                </feMerge>
              </filter>
            </defs>

            <rect class="chip-board" x="24" y="24" width="472" height="112" rx="14"></rect>

            <g class="chip-grid">
              <line x1="48" y1="48" x2="472" y2="48"></line>
              <line x1="48" y1="80" x2="472" y2="80"></line>
              <line x1="48" y1="112" x2="472" y2="112"></line>
              <line x1="92" y1="34" x2="92" y2="126"></line>
              <line x1="176" y1="34" x2="176" y2="126"></line>
              <line x1="260" y1="34" x2="260" y2="126"></line>
              <line x1="344" y1="34" x2="344" y2="126"></line>
              <line x1="428" y1="34" x2="428" y2="126"></line>
            </g>

            <g class="chip-traces">
              <path class="chip-trace flow-east" d="M48 48 H176 V64 H220"></path>
              <path class="chip-trace flow-east" d="M48 112 H176 V96 H220"></path>
              <path class="chip-trace flow-west" d="M472 48 H344 V64 H300"></path>
              <path class="chip-trace flow-west" d="M472 112 H344 V96 H300"></path>
              <path class="chip-trace flow-south" d="M176 34 V56 H220"></path>
              <path class="chip-trace flow-south" d="M344 34 V56 H300"></path>
              <path class="chip-trace flow-north" d="M176 126 V104 H220"></path>
              <path class="chip-trace flow-north" d="M344 126 V104 H300"></path>
            </g>

            <g class="chip-ports">
              <circle cx="48" cy="48" r="4"></circle>
              <circle cx="48" cy="112" r="4"></circle>
              <circle cx="472" cy="48" r="4"></circle>
              <circle cx="472" cy="112" r="4"></circle>
              <circle cx="176" cy="34" r="4"></circle>
              <circle cx="344" cy="34" r="4"></circle>
              <circle cx="176" cy="126" r="4"></circle>
              <circle cx="344" cy="126" r="4"></circle>
            </g>

            <rect class="chip-core-shell" x="220" y="56" width="80" height="48" rx="8"></rect>
            <rect class="chip-core" x="232" y="68" width="56" height="24" rx="4"></rect>
            <line class="chip-core-wire" x1="232" y1="80" x2="288" y2="80"></line>
            <line class="chip-core-wire" x1="260" y1="68" x2="260" y2="92"></line>
            <circle class="chip-core-pulse" cx="260" cy="80" r="12"></circle>

            <g class="chip-packet packet-east-a">
              <circle cx="0" cy="0" r="2.4"></circle>
            </g>
            <g class="chip-packet packet-east-b">
              <circle cx="0" cy="0" r="2.4"></circle>
            </g>
            <g class="chip-packet packet-west-a">
              <circle cx="0" cy="0" r="2.4"></circle>
            </g>
            <g class="chip-packet packet-west-b">
              <circle cx="0" cy="0" r="2.4"></circle>
            </g>
            <g class="chip-packet packet-south">
              <circle cx="0" cy="0" r="2.2"></circle>
            </g>
            <g class="chip-packet packet-north">
              <circle cx="0" cy="0" r="2.2"></circle>
            </g>
          </svg>
        </div>
        <h1 class="mb-1 text-4xl font-semibold tracking-tight">${escapeHtml(state.controlPanelName)}</h1>
        <p class="tcp-subtle mb-6">Use your engine API token to unlock the full web control center.</p>
        <form id="login-form" class="grid gap-3">
          <label class="text-sm text-slate-300">Engine Token</label>
          <input id="token" class="tcp-input" type="password" placeholder="tk_..." autocomplete="off" value="${escapeHtml(savedToken)}" />
          <label class="inline-flex items-center gap-2 text-xs text-slate-400">
            <input id="remember-token" type="checkbox" class="h-4 w-4 accent-slate-400" checked />
            Remember token on this browser
          </label>
          <button id="login-btn" type="submit" class="tcp-btn-primary w-full"><i data-lucide="key-round"></i> Sign In</button>
          <button id="check-engine-btn" type="button" class="tcp-btn w-full"><i data-lucide="activity"></i> Check Engine Connectivity</button>
          <div id="login-err" class="min-h-[1.2rem] text-sm text-rose-300"></div>
        </form>
      </section>
    </main>
  `;

  renderIcons();

  byId("login-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const token = byId("token").value.trim();
    const remember = !!byId("remember-token")?.checked;
    const errEl = byId("login-err");
    errEl.textContent = "";

    if (!token) {
      errEl.textContent = "Token is required.";
      toast("warn", "Engine token is required.");
      return;
    }

    try {
      await api("/api/auth/login", {
        method: "POST",
        body: JSON.stringify({ token }),
      });
      if (remember) saveToken(token);
      else clearSavedToken();
      await checkAuth();
      toast("ok", "Signed in.");
      setRoute("dashboard");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      errEl.textContent = message;
      toast("err", message);
    }
  });

  byId("check-engine-btn").addEventListener("click", async () => {
    const errEl = byId("login-err");
    errEl.textContent = "";
    try {
      const health = await api("/api/system/health");
      const stateText = health.engine?.ready || health.engine?.healthy ? "healthy" : "unhealthy";
      errEl.textContent = `Engine check: ${stateText} at ${health.engineUrl}`;
      errEl.className = "min-h-[1.2rem] text-sm text-lime-300";
      toast(
        health.engine?.ready || health.engine?.healthy ? "ok" : "warn",
        `Engine ${stateText}: ${health.engineUrl}`
      );
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      errEl.textContent = message;
      errEl.className = "min-h-[1.2rem] text-sm text-rose-300";
      toast("err", message);
    }
  });
}

async function renderRoute(options = {}) {
  const { showLoading = true, animateView = true } = options;
  const renderSeq = ++routeRenderSeq;
  const view = byId("view");
  if (!view) return;
  if (showLoading) {
    view.innerHTML = '<div class="tcp-subtle">Loading...</div>';
  }

  const providerRequiredRoutes = new Set(["chat", "agents", "swarm", "teams"]);
  if (providerRequiredRoutes.has(state.route) && !state.providerReady) {
    view.innerHTML = `
      <div class="tcp-card">
        <h3 class="tcp-title mb-2">Provider Setup Required</h3>
        <p class="tcp-subtle">This page requires a connected default provider/model before runs can execute.</p>
        <div class="mt-3 flex flex-wrap gap-2">
          <span class="tcp-badge-warn">default: ${escapeHtml(state.providerDefault || "none")}</span>
          <span class="tcp-badge-warn">connected: ${escapeHtml(String(state.providerConnected.length))}</span>
        </div>
        <div class="mt-4 flex justify-end">
          <button id="goto-settings" class="tcp-btn-primary">Open Provider Setup</button>
        </div>
      </div>
    `;
    const btn = byId("goto-settings");
    if (btn) btn.addEventListener("click", () => setRoute("settings"));
    return;
  }

  const renderer = VIEW_RENDERERS[state.route] || VIEW_RENDERERS.dashboard;
  await renderer(ctx);
  if (renderSeq !== routeRenderSeq) return;
  renderIcons(byId("view"));
  if (animateView) {
    animateRouteView(view);
    animateNav();
  }
}

function animateRouteView(view) {
  try {
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches) return;
    animate(view, { opacity: [0.72, 1] }, { duration: 0.16, easing: "ease-out" });
    const items = [...view.querySelectorAll(".tcp-card, .tcp-panel, .tcp-list-item")].slice(0, 24);
    if (!items.length) return;
    animate(
      items,
      { opacity: [0, 1], transform: ["translateY(8px)", "translateY(0px)"] },
      { duration: 0.2, easing: "ease-out", delay: stagger(0.02) }
    );
  } catch {
    // ignore animation failures
  }
}

function animateNav() {
  try {
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches) return;
    const active = document.querySelector("#nav .nav-item.active");
    if (!active) return;
    animate(
      active,
      { opacity: [0.72, 1], transform: ["translateX(-3px)", "translateX(0px)"] },
      { duration: 0.16, easing: "ease-out" }
    );
  } catch {
    // ignore animation failures
  }
}

function renderShell() {
  if (!state.authed) {
    renderLogin();
    return;
  }

  clearCleanup();

  app.innerHTML = `
    <div class="grid min-h-screen grid-cols-1 lg:grid-cols-[260px_1fr]">
      <aside class="border-r border-slate-700 bg-panel/90 p-4">
        <div class="mb-4 flex items-center gap-3 rounded-xl border border-slate-700 bg-black/20 p-3">
          <div class="grid h-10 w-10 place-items-center overflow-hidden rounded-xl border border-slate-600 bg-muted text-slate-200">
            ${
              state.botAvatarUrl
                ? `<img src="${escapeHtml(state.botAvatarUrl)}" alt="${escapeHtml(state.botName)}" class="h-full w-full object-cover" />`
                : `<i data-lucide="cpu"></i>`
            }
          </div>
          <div>
            <div class="text-base font-semibold">${escapeHtml(state.botName)}</div>
            <div class="text-xs uppercase tracking-wider text-slate-400">Control Center</div>
          </div>
        </div>
        <nav id="nav" class="grid gap-1"></nav>
        <div class="mt-4 border-t border-slate-700 pt-4">
          <button id="logout-btn" class="tcp-btn w-full"><i data-lucide="log-out"></i> Logout</button>
        </div>
      </aside>
      <main class="min-w-0 p-3 md:p-4">
        <section id="view" class="grid h-full gap-4"></section>
      </main>
    </div>
  `;

  const nav = byId("nav");
  nav.innerHTML = NAV_ROUTES.map(
    ([id, label, icon]) => `
      <button data-route="${id}" class="nav-item ${id === state.route ? "active" : ""}">
        <i data-lucide="${icon}"></i><span>${label}</span>
      </button>
    `
  ).join("");

  nav.querySelectorAll(".nav-item").forEach((btn) => {
    btn.addEventListener("click", () => setRoute(btn.dataset.route));
  });

  byId("logout-btn").addEventListener("click", async () => {
    await api("/api/auth/logout", { method: "POST" }).catch(() => {});
    state.authed = false;
    state.me = null;
    state.client = null;
    renderLogin();
  });

  renderIcons(app);
  renderToasts();
  void renderRoute();
}

async function renderDashboardIfAuthLost() {
  try {
    await api("/api/auth/me");
  } catch {
    state.authed = false;
    renderLogin();
  }
}

window.addEventListener("hashchange", () => {
  const nextRoute = ensureRoute(routeFromHash(), ROUTES);
  const routeChanged = nextRoute !== state.route;
  state.route = nextRoute;
  if (!state.authed) {
    renderLogin();
    return;
  }
  if (routeChanged) {
    renderShell();
    return;
  }
  clearCleanup();
  void renderRoute({ showLoading: false, animateView: false });
});

async function boot() {
  state.themeId = getActiveThemeId();
  applyTheme(state.themeId);
  state.route = ensureRoute(routeFromHash(), ROUTES);
  await checkAuth();
  if (!state.authed) {
    const savedToken = getSavedToken().trim();
    if (savedToken) {
      try {
        await api("/api/auth/login", {
          method: "POST",
          body: JSON.stringify({ token: savedToken }),
        });
        await checkAuth();
      } catch {
        clearSavedToken();
      }
    }
  }
  if (!state.authed) return renderLogin();

  renderShell();
  if (state.needsProviderOnboarding && state.route === "dashboard") {
    toast("info", "Complete provider setup to start using chat/agents.");
    setRoute("settings");
  }

  const authPoll = setInterval(renderDashboardIfAuthLost, 30000);
  addCleanup(() => clearInterval(authPoll));
}

boot();
