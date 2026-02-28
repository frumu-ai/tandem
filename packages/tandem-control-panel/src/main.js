import { TandemClient } from "@frumu/tandem-client";
import "./styles.css";
import { api } from "./app/api.js";
import { byId, escapeHtml } from "./app/dom.js";
import { routeFromHash, ensureRoute, setHashRoute } from "./app/router.js";
import { createToasts } from "./app/toasts.js";
import { createState, ROUTES, providerHints } from "./app/store.js";
import { VIEW_RENDERERS } from "./views/index.js";
import { renderIcons } from "./app/icons.js";

const app = document.getElementById("app");
const state = createState();
const { toast, renderToasts } = createToasts(state);
const TOKEN_STORAGE_KEY = "tandem_control_panel_token";

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
  renderIcons,
};

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
    await refreshProviderStatus();
  } catch {
    state.authed = false;
    state.me = null;
    state.client = null;
    state.needsProviderOnboarding = false;
    state.providerReady = false;
    state.providerDefault = "";
    state.providerConnected = [];
    state.providerError = "";
  }
}

async function refreshProviderStatus() {
  if (!state.client) {
    state.needsProviderOnboarding = false;
    state.providerReady = false;
    state.providerDefault = "";
    state.providerConnected = [];
    state.providerError = "";
    return;
  }
  try {
    const [config, catalog] = await Promise.all([state.client.providers.config(), state.client.providers.catalog()]);
    const defaultProvider = String(config?.default || "").trim();
    const connected = new Set(catalog?.connected || []);
    const ready = !!defaultProvider && (defaultProvider === "ollama" || connected.has(defaultProvider));
    state.providerDefault = defaultProvider;
    state.providerConnected = [...connected];
    state.providerReady = ready;
    state.providerError = "";
    state.needsProviderOnboarding = !ready;
  } catch (e) {
    state.providerReady = false;
    state.providerDefault = "";
    state.providerConnected = [];
    state.providerError = e instanceof Error ? e.message : String(e);
    state.needsProviderOnboarding = true;
  }
}

function setRoute(route) {
  state.route = ensureRoute(route, ROUTES);
  setHashRoute(state.route);
  renderShell();
}

function renderLogin() {
  const savedToken = getSavedToken();
  app.innerHTML = `
    <main class="mx-auto grid min-h-screen w-full max-w-3xl place-items-center p-5">
      <section class="tcp-panel w-full max-w-xl">
        <div class="mb-6 rounded-2xl border border-slate-700 bg-black/20 p-3">
          <svg viewBox="0 0 520 160" class="hero-svg" aria-hidden="true">
            <defs>
              <linearGradient id="hero-path-grad" x1="0" y1="0" x2="1" y2="0">
                <stop offset="0%" stop-color="#64748b" stop-opacity="0.35"></stop>
                <stop offset="50%" stop-color="#cbd5e1" stop-opacity="0.95"></stop>
                <stop offset="100%" stop-color="#64748b" stop-opacity="0.35"></stop>
              </linearGradient>
              <radialGradient id="hero-core-grad" cx="50%" cy="50%" r="60%">
                <stop offset="0%" stop-color="#f1f5f9" stop-opacity="0.9"></stop>
                <stop offset="100%" stop-color="#64748b" stop-opacity="0.15"></stop>
              </radialGradient>
            </defs>

            <g class="hero-grid">
              <line x1="24" y1="34" x2="496" y2="34"></line>
              <line x1="24" y1="80" x2="496" y2="80"></line>
              <line x1="24" y1="126" x2="496" y2="126"></line>
              <line x1="120" y1="24" x2="120" y2="136"></line>
              <line x1="260" y1="24" x2="260" y2="136"></line>
              <line x1="400" y1="24" x2="400" y2="136"></line>
            </g>

            <path class="hero-path hero-path-left" d="M60 92 C110 92, 150 78, 194 80 S238 80, 260 80"></path>
            <path class="hero-path hero-path-right" d="M460 68 C410 68, 370 82, 326 80 S282 80, 260 80"></path>
            <path class="hero-path hero-path-upper" d="M88 52 C150 52, 200 58, 260 58 S370 58, 432 52"></path>

            <g class="hero-node hero-node-left">
              <circle cx="60" cy="92" r="8"></circle>
            </g>
            <g class="hero-node hero-node-right">
              <circle cx="460" cy="68" r="8"></circle>
            </g>
            <g class="hero-node hero-node-top-left">
              <circle cx="88" cy="52" r="6"></circle>
            </g>
            <g class="hero-node hero-node-top-right">
              <circle cx="432" cy="52" r="6"></circle>
            </g>

            <circle class="hero-core-glow" cx="260" cy="80" r="28"></circle>
            <circle class="hero-core-ring" cx="260" cy="80" r="24"></circle>
            <circle class="hero-core" cx="260" cy="80" r="12"></circle>
            <circle class="hero-scan" cx="260" cy="80" r="34"></circle>

            <g class="hero-packet hero-packet-left">
              <circle cx="0" cy="0" r="3"></circle>
            </g>
            <g class="hero-packet hero-packet-right">
              <circle cx="0" cy="0" r="3"></circle>
            </g>
            <g class="hero-packet hero-packet-upper">
              <circle cx="0" cy="0" r="2.5"></circle>
            </g>
          </svg>
        </div>
        <h1 class="mb-1 text-4xl font-semibold tracking-tight">Tandem Control Panel</h1>
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
      toast(health.engine?.ready || health.engine?.healthy ? "ok" : "warn", `Engine ${stateText}: ${health.engineUrl}`);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      errEl.textContent = message;
      errEl.className = "min-h-[1.2rem] text-sm text-rose-300";
      toast("err", message);
    }
  });
}

async function renderRoute() {
  const view = byId("view");
  if (!view) return;
  view.innerHTML = '<div class="tcp-subtle">Loading...</div>';

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
  renderIcons(byId("view"));
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
          <div class="grid h-10 w-10 place-items-center rounded-xl border border-slate-600 bg-muted text-slate-200"><i data-lucide="cpu"></i></div>
          <div>
            <div class="text-base font-semibold">Tandem</div>
            <div class="text-xs uppercase tracking-wider text-slate-400">Control Center</div>
          </div>
        </div>
        <nav id="nav" class="grid gap-1"></nav>
        <div class="mt-4 border-t border-slate-700 pt-4">
          <button id="logout-btn" class="tcp-btn w-full"><i data-lucide="log-out"></i> Logout</button>
        </div>
      </aside>
      <main class="min-w-0 p-4 md:p-6">
        <header class="tcp-panel mb-4 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 class="text-xl font-semibold tracking-tight">${escapeHtml((ROUTES.find((r) => r[0] === state.route) || ["", "Dashboard"])[1])}</h2>
            <p class="mt-1 font-mono text-xs text-slate-400">${escapeHtml(state.me?.engineUrl || "")}</p>
          </div>
          <span class="${state.me?.localEngine ? "tcp-badge-ok" : "tcp-badge-warn"}">${state.me?.localEngine ? "Local Engine" : "Remote Engine"}</span>
        </header>
        <section id="view" class="grid gap-4"></section>
      </main>
    </div>
  `;

  const nav = byId("nav");
  nav.innerHTML = ROUTES.map(
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
  state.route = ensureRoute(routeFromHash(), ROUTES);
  renderShell();
});

async function boot() {
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
