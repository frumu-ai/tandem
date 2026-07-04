import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { motion } from "motion/react";
import { TandemClient } from "@frumu/tandem-client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { LoginPage } from "./LoginPage";
import { CommandPalette, usePaletteHotkey, type PaletteAction } from "./CommandPalette";
import { APP_NAV_ROUTES, APP_ROUTES } from "./routes";
import { useHashRoute } from "./useHashRoute";
import { ToastProvider, useToast } from "./toast";
import { HashRouteOutlet } from "./HashRouteOutlet";
import { AppShell } from "./AppShell";
import { deriveProviderState } from "./providerStatus";
import { providerHints } from "./store.js";
import {
  getDefaultNavigationVisibility,
  loadNavigationVisibility,
  saveNavigationVisibility,
  visibleNavigationRoutes,
  type NavigationVisibility,
} from "./navigation";
import {
  THEMES,
  applyTheme,
  cycleThemeId,
  getActiveThemeId,
  setControlPanelTheme,
} from "./themes.js";
import { renderIcons } from "./icons.js";
import { api, isTransientEngineError } from "../lib/api";
import { useCapabilities, useSwarmStatus, useSystemHealth } from "../features/system/queries";
import { PanelCard, StatusPulse } from "../ui/index.tsx";
import type { RouteId } from "./routes";
import type { NavigationLockState } from "../pages/pageTypes";

const TOKEN_STORAGE_KEY = "tandem_control_panel_token";
const PALETTE_HIDDEN_ROUTE_IDS = new Set<RouteId>([
  "packs",
  "teams",
  "channels",
  "mcp",
  "files",
  "packs-detail",
  "teams-detail",
]);

function getSavedToken() {
  try {
    return localStorage.getItem(TOKEN_STORAGE_KEY) || "";
  } catch {
    return "";
  }
}

function saveToken(token: string) {
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

function useProviderStatus(client: TandemClient | null, enabled: boolean) {
  return useQuery({
    queryKey: ["provider", "status"],
    enabled: enabled && !!client,
    refetchInterval: enabled ? 15000 : false,
    queryFn: async () => {
      if (!client) {
        return {
          ready: false,
          defaultProvider: "",
          defaultModel: "",
          connected: [],
          error: "",
          needsOnboarding: false,
          defaultProviderAuthKind: "",
          defaultProviderSource: "",
          defaultProviderManagedBy: "",
        };
      }
      try {
        const [config, authStatus] = await Promise.all([
          client.providers.config(),
          client.providers.authStatus().catch(() => ({})),
        ]);
        return deriveProviderState(config, null, authStatus);
      } catch (error) {
        // A transient backend/network failure should not hard-lock the app back
        // onto Settings. Keep the error visible, but avoid treating it as an
        // onboarding requirement unless we successfully fetched provider state.
        return {
          ready: false,
          defaultProvider: "",
          defaultModel: "",
          connected: [],
          error: error instanceof Error ? error.message : String(error),
          needsOnboarding: false,
          defaultProviderAuthKind: "",
          defaultProviderSource: "",
          defaultProviderManagedBy: "",
        };
      }
    },
  });
}

function useIdentity(client: TandemClient | null, enabled: boolean) {
  return useQuery({
    queryKey: ["identity"],
    enabled: enabled && !!client,
    refetchInterval: enabled ? 30000 : false,
    queryFn: async () => {
      if (!client) {
        return { botName: "Tandem", botAvatarUrl: "", controlPanelName: "Tandem Control Panel" };
      }
      try {
        const payload = await api("/api/engine/config/identity", { method: "GET" });
        const identity = (payload as any)?.identity || {};
        const canonical = String(
          identity?.bot?.canonical_name || identity?.bot?.canonicalName || ""
        ).trim();
        const aliases = identity?.bot?.aliases || {};
        const avatar = String(identity?.bot?.avatar_url || identity?.bot?.avatarUrl || "").trim();
        const controlPanelAlias = String(
          aliases?.control_panel || aliases?.controlPanel || ""
        ).trim();
        const botName = canonical || "Tandem";
        return {
          botName,
          botAvatarUrl: avatar,
          controlPanelName: controlPanelAlias || `${botName} Control Panel`,
        };
      } catch {
        return { botName: "Tandem", botAvatarUrl: "", controlPanelName: "Tandem Control Panel" };
      }
    },
  });
}

function useIncidentMonitorStatus(enabled: boolean) {
  return useQuery({
    queryKey: ["incident-monitor", "status"],
    enabled,
    refetchInterval: enabled ? 10000 : false,
    queryFn: async () => {
      try {
        return await api("/api/engine/incident-monitor/status", { method: "GET" });
      } catch {
        return null;
      }
    },
  });
}

function usePendingApprovals(enabled: boolean) {
  return useQuery({
    queryKey: ["approvals", "pending", "count"],
    enabled,
    refetchInterval: enabled ? 5000 : false,
    queryFn: async () => {
      try {
        const payload = await api("/api/engine/approvals/pending", { method: "GET" });
        const approvals = Array.isArray((payload as any)?.approvals)
          ? (payload as any).approvals
          : [];
        const count = Number((payload as any)?.count);
        return {
          count: Number.isFinite(count) ? count : approvals.length,
        };
      } catch {
        return { count: 0 };
      }
    },
  });
}

function ReconnectingPage({
  controlPanelName,
  controlPanelMode,
  controlPanelModeReason,
  errorMessage,
  onRetry,
  onCheckEngine,
}: {
  controlPanelName: string;
  controlPanelMode?: string;
  controlPanelModeReason?: string;
  errorMessage: string;
  onRetry: () => void;
  onCheckEngine: () => Promise<string>;
}) {
  const [message, setMessage] = useState("");
  const [ok, setOk] = useState(false);

  return (
    <main className="relative min-h-screen overflow-hidden px-5 py-8">
      <div className="relative z-10 mx-auto grid min-h-[calc(100vh-4rem)] w-full max-w-6xl items-center gap-6 lg:grid-cols-[1.05fr_0.95fr]">
        <section className="grid gap-4">
          <div className="tcp-page-eyebrow">Tandem Control</div>
          <h1 className="tcp-page-title max-w-3xl">Reconnecting your session.</h1>
          <p className="tcp-subtle max-w-2xl text-base">
            The control panel still has your session, but the engine is temporarily unavailable.
            We&apos;ll reconnect automatically as soon as it responds again.
          </p>
        </section>

        <PanelCard
          title={controlPanelName}
          subtitle={
            controlPanelMode === "aca"
              ? "ACA install detected. Waiting for the Tandem engine to restore the existing session."
              : "Standalone install detected. Waiting for the Tandem engine to restore the existing session."
          }
        >
          <div className="grid gap-3">
            <div className="rounded-xl border border-slate-700/60 bg-slate-950/30 p-3">
              <div className="mb-2 flex items-center justify-between gap-3">
                <div className="font-medium">Session recovery</div>
                <StatusPulse
                  tone={ok ? "ok" : "warn"}
                  text={ok ? "Engine reachable" : "Reconnecting"}
                />
              </div>
              <p className="tcp-subtle text-xs">
                Existing auth is intact. The panel is waiting for the engine before reloading the
                app shell.
              </p>
              {controlPanelModeReason ? (
                <p className="tcp-subtle mt-2 text-xs">{controlPanelModeReason}</p>
              ) : null}
            </div>

            <div className="grid gap-2 sm:grid-cols-2">
              <button type="button" className="tcp-btn-primary w-full" onClick={onRetry}>
                <i data-lucide="refresh-cw"></i>
                Retry now
              </button>
              <button
                type="button"
                className="tcp-btn w-full"
                onClick={async () => {
                  try {
                    const result = await onCheckEngine();
                    setOk(true);
                    setMessage(result);
                  } catch (error) {
                    setOk(false);
                    setMessage(error instanceof Error ? error.message : String(error));
                  }
                }}
              >
                <i data-lucide="activity"></i>
                Check engine
              </button>
            </div>

            <div className={`min-h-[1.2rem] text-sm ${ok ? "text-lime-300" : "text-amber-200"}`}>
              {message || errorMessage}
            </div>
          </div>
        </PanelCard>
      </div>
    </main>
  );
}

function AppBody() {
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const [themeId, setThemeId] = useState(getActiveThemeId());
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [navigationLock, setNavigationLock] = useState<NavigationLockState | null>(null);
  const [providerGateNoticeShown, setProviderGateNoticeShown] = useState(false);
  const autoLoginAttempted = useRef(false);
  const savedBootstrapTokenRef = useRef<string | null>(null);
  if (savedBootstrapTokenRef.current === null) {
    savedBootstrapTokenRef.current = getSavedToken().trim();
  }
  const hostedCodeRef = useRef<string | null>(null);
  if (hostedCodeRef.current === null && typeof window !== "undefined") {
    hostedCodeRef.current = new URLSearchParams(window.location.search).get("hosted_code") || "";
  }
  const [authBootstrapPending, setAuthBootstrapPending] = useState(
    () => !!savedBootstrapTokenRef.current || !!hostedCodeRef.current
  );
  const { route, navigate } = useHashRoute({
    canNavigate: useCallback(
      (next, current) => !navigationLock || next === current,
      [navigationLock]
    ),
  });

  useEffect(() => {
    applyTheme(themeId);
  }, [themeId]);

  const authQuery = useQuery({
    queryKey: ["auth", "me"],
    enabled: !authBootstrapPending,
    retry: (failureCount, error) => isTransientEngineError(error) && failureCount < 4,
    retryDelay: (attemptIndex) => Math.min(1000 * 2 ** attemptIndex, 5000),
    refetchInterval: (query) =>
      isTransientEngineError(query.state.error) ? 5000 : query.state.data ? 30000 : false,
    refetchOnWindowFocus: true,
    queryFn: () => api("/api/auth/me", { method: "GET", cache: "no-store" }),
  });

  const authTransient = isTransientEngineError(authQuery.error);
  const authed = authQuery.data?.ok === true;
  const client = useMemo(
    () => (authed ? new TandemClient({ baseUrl: "/api/engine", token: "session" }) : null),
    [authed]
  );

  const providerQuery = useProviderStatus(client, authed);
  const identityQuery = useIdentity(client, authed);
  const healthQuery = useSystemHealth(authed);
  const swarmStatusQuery = useSwarmStatus(authed);
  const incidentMonitorQuery = useIncidentMonitorStatus(authed);
  const pendingApprovalsQuery = usePendingApprovals(authed);
  const capabilitiesQuery = useCapabilities();
  const controlPanelMode =
    String(
      capabilitiesQuery.data?.control_panel_mode ||
        (capabilitiesQuery.data?.aca_integration ? "aca" : "standalone")
    ).trim() || "standalone";
  const hostedManaged = capabilitiesQuery.data?.hosted_managed === true;
  const hostedAuthAvailable =
    hostedManaged &&
    capabilitiesQuery.data?.hosted_auth_available !== false &&
    !!String(capabilitiesQuery.data?.hosted_panel_login_url || "").trim();
  const acaMode = controlPanelMode === "aca";
  const navVisibilityHydrated = useRef(false);
  const [navVisibility, setNavVisibility] = useState<NavigationVisibility>(() =>
    loadNavigationVisibility(false)
  );

  useEffect(() => {
    if (!authed || !capabilitiesQuery.isFetched || navVisibilityHydrated.current) return;
    navVisibilityHydrated.current = true;
    setNavVisibility(loadNavigationVisibility(acaMode));
  }, [acaMode, authed, capabilitiesQuery.isFetched]);

  useEffect(() => {
    if (!navVisibilityHydrated.current) return;
    saveNavigationVisibility(navVisibility);
  }, [navVisibility]);

  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  }, [authed, navVisibility, route]);

  // Icons are `<i data-lucide>` placeholders that lucide replaces with SVGs.
  // The route-change scan above fires before async query data mounts its own
  // icons, so late-arriving `<i data-lucide>` nodes (list rows, cards, buttons
  // gated on query results) would otherwise stay blank. A single observer
  // re-renders whenever new placeholders appear, covering every page without
  // each one having to wire up its own renderIcons call.
  //
  // Match ONLY `<i data-lucide>` placeholders, not any `[data-lucide]`: lucide's
  // generated `<svg>` keeps the data-lucide attribute, so a broader match would
  // see each rendered icon as pending and re-render it every frame (a runaway
  // observer/rAF loop). Placeholders are `<i>`, the output is `<svg>`, so keying
  // on the tag name breaks the loop.
  useEffect(() => {
    if (typeof MutationObserver === "undefined") return;
    const hasPendingIcon = (node: Node) => {
      if (!(node instanceof Element)) return false;
      return node.matches("i[data-lucide]") || node.querySelector("i[data-lucide]") !== null;
    };
    const observer = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        for (const node of mutation.addedNodes) {
          if (hasPendingIcon(node)) {
            renderIcons();
            return;
          }
        }
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!navigationLock) return;
    if (route === "automations") return;
    setNavigationLock(null);
  }, [navigationLock, route]);

  useEffect(() => {
    if (!navigationLock) return;
    setPaletteOpen(false);
  }, [navigationLock]);

  useEffect(() => {
    if (!navigationLock) return undefined;
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
      return "";
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [navigationLock]);

  const loginMutation = useMutation({
    mutationFn: async ({ token }: { token: string; remember: boolean }) => {
      await api("/api/auth/login", {
        method: "POST",
        body: JSON.stringify({ token }),
      });
    },
    onSuccess: (_, vars) => {
      if (vars.remember) saveToken(vars.token);
      else clearSavedToken();
      queryClient.invalidateQueries({ queryKey: ["auth", "me"] });
      toast("ok", "Signed in.");
      navigate("dashboard");
    },
    onError: (error) => {
      const message = error instanceof Error ? error.message : String(error);
      toast(isTransientEngineError(error) ? "info" : "err", message);
    },
  });

  useEffect(() => {
    if (authBootstrapPending) return;
    if (authed || authTransient || loginMutation.isPending || autoLoginAttempted.current) return;
    const savedToken = getSavedToken().trim();
    if (!savedToken) {
      autoLoginAttempted.current = true;
      return;
    }
    autoLoginAttempted.current = true;
    loginMutation.mutate({ token: savedToken, remember: true });
  }, [authBootstrapPending, authTransient, authed, loginMutation]);

  useEffect(() => {
    const hostedCode = hostedCodeRef.current || "";
    if (!hostedCode) return;
    if (loginMutation.isPending) return;
    hostedCodeRef.current = "";
    autoLoginAttempted.current = true;
    api("/api/auth/hosted/exchange", {
      method: "POST",
      body: JSON.stringify({ code: hostedCode }),
    })
      .then(() => {
        const url = new URL(window.location.href);
        url.searchParams.delete("hosted_code");
        window.history.replaceState({}, "", `${url.pathname}${url.search}${url.hash}`);
        clearSavedToken();
        return queryClient.invalidateQueries({ queryKey: ["auth", "me"] });
      })
      .catch((error) => {
        toast("err", error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        setAuthBootstrapPending(false);
      });
  }, [loginMutation.isPending, queryClient, toast]);

  useEffect(() => {
    if (!authBootstrapPending || loginMutation.isPending) return;
    if (hostedCodeRef.current) return;
    const savedToken = savedBootstrapTokenRef.current || "";
    autoLoginAttempted.current = true;
    if (!savedToken) {
      setAuthBootstrapPending(false);
      return;
    }
    loginMutation.mutate(
      { token: savedToken, remember: true },
      {
        onError: () => {
          clearSavedToken();
          savedBootstrapTokenRef.current = "";
        },
        onSettled: () => {
          setAuthBootstrapPending(false);
        },
      }
    );
  }, [authBootstrapPending, loginMutation]);

  const logout = useCallback(async () => {
    await api("/api/auth/logout", { method: "POST" }).catch(() => {});
    queryClient.removeQueries({ queryKey: ["auth"] });
    queryClient.removeQueries({ queryKey: ["control-panel"] });
    queryClient.removeQueries({ queryKey: ["provider"] });
    queryClient.removeQueries({ queryKey: ["identity"] });
    queryClient.invalidateQueries({ queryKey: ["auth", "me"] });
    toast("info", "Logged out.");
  }, [queryClient, toast]);

  const lockedRoutes = useMemo(
    () => new Set(["chat", "planner", "studio", "agents", "orchestrator", "teams", "experiments"]),
    []
  );
  const needsProviderOnboarding = !!providerQuery.data?.needsOnboarding;
  const providerLocked = authed && needsProviderOnboarding;

  // The provider-setup prompt is a dismissible banner, not a floating modal on
  // every route. Dismissal is session-scoped: it stays hidden while the user
  // works, and returns next session if provider setup is still incomplete. The
  // persistent header pill keeps the status visible after dismissal (TAN-588).
  const [providerNoticeDismissed, setProviderNoticeDismissed] = useState(() => {
    try {
      return sessionStorage.getItem("tandem.providerNoticeDismissed") === "1";
    } catch {
      return false;
    }
  });
  const dismissProviderNotice = useCallback(() => {
    setProviderNoticeDismissed(true);
    try {
      sessionStorage.setItem("tandem.providerNoticeDismissed", "1");
    } catch {
      // ignore storage failures
    }
  }, []);

  useEffect(() => {
    if (!providerLocked) {
      setProviderGateNoticeShown(false);
      return;
    }
    if (!lockedRoutes.has(route)) return;
    navigate("settings");
    if (!providerGateNoticeShown) {
      setProviderGateNoticeShown(true);
      toast("info", "Set provider + default model in Providers first to unlock the control panel.");
    }
  }, [lockedRoutes, navigate, providerGateNoticeShown, providerLocked, route, toast]);

  const currentRoute = providerLocked && lockedRoutes.has(route) ? "settings" : route;

  const refreshProviderStatus = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["provider", "status"] });
  }, [queryClient]);

  const refreshIdentityStatus = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["identity"] });
  }, [queryClient]);

  const setRouteVisibility = useCallback((routeId: RouteId, visible: boolean) => {
    setNavVisibility((current) => {
      const next = { ...current, [routeId]: visible };
      return next[routeId] === current[routeId] ? current : next;
    });
  }, []);

  const showAllSections = useCallback(() => {
    setNavVisibility((current) => {
      const next = { ...current };
      for (const [routeId] of APP_NAV_ROUTES) {
        next[routeId as RouteId] = true;
      }
      return next;
    });
  }, []);

  const resetNavigation = useCallback(() => {
    setNavVisibility(getDefaultNavigationVisibility(acaMode));
  }, [acaMode]);

  const navRoutes = useMemo(() => visibleNavigationRoutes(navVisibility), [navVisibility]);
  const navigation = useMemo(
    () => ({
      acaMode,
      routeVisibility: navVisibility,
      setRouteVisibility,
      showAllSections,
      resetNavigation,
    }),
    [acaMode, navVisibility, resetNavigation, setRouteVisibility, showAllSections]
  );

  const setTheme = useCallback(
    (nextThemeId: string) => {
      const theme = setControlPanelTheme(nextThemeId);
      setThemeId(theme.id);
      return theme;
    },
    [setThemeId]
  );

  const identity = identityQuery.data || {
    botName: "Tandem",
    botAvatarUrl: "",
    controlPanelName: "Tandem Control Panel",
  };

  const commonPageProps = {
    client: client!,
    api,
    toast,
    navigate,
    currentRoute,
    providerStatus: {
      ready: !!providerQuery.data?.ready,
      defaultProvider: providerQuery.data?.defaultProvider || "",
      defaultModel: providerQuery.data?.defaultModel || "",
      connected: providerQuery.data?.connected || [],
      error: providerQuery.data?.error || "",
      needsOnboarding: !!providerQuery.data?.needsOnboarding,
      defaultProviderAuthKind: providerQuery.data?.defaultProviderAuthKind || "",
      defaultProviderSource: providerQuery.data?.defaultProviderSource || "",
      defaultProviderManagedBy: providerQuery.data?.defaultProviderManagedBy || "",
    },
    identity,
    refreshProviderStatus,
    refreshIdentityStatus,
    providerHints,
    themes: THEMES,
    setTheme,
    themeId,
    navigation,
    navigationLock,
    setNavigationLock,
  };

  const paletteActions = useMemo<PaletteAction[]>(() => {
    const routeActions = APP_ROUTES.filter(
      ([id]) =>
        navVisibility[id as RouteId] !== false && !PALETTE_HIDDEN_ROUTE_IDS.has(id as RouteId)
    ).map(([id, label]) => ({
      id: `route:${id}`,
      label: `Go to ${label}`,
      group: "Routes",
      onSelect: () => navigate(id),
    }));

    const engineActions: PaletteAction[] = [
      {
        id: "action:new-chat",
        label: "New chat session",
        group: "Actions",
        onSelect: () => {
          window.dispatchEvent(new CustomEvent("tcp:new-chat"));
          navigate("chat");
        },
      },
      {
        id: "action:start-engine-check",
        label: "Check engine health",
        group: "Actions",
        onSelect: async () => {
          try {
            const health = await api("/api/system/health");
            const status =
              health?.engine?.ready || health?.engine?.healthy ? "healthy" : "unhealthy";
            toast("info", `Engine ${status}: ${health?.engineUrl || "n/a"}`);
          } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            toast(isTransientEngineError(error) ? "info" : "err", message);
          }
        },
      },
      {
        id: "action:open-settings",
        label: "Open provider settings",
        group: "Actions",
        onSelect: () => navigate("settings"),
      },
      ...(navVisibility.orchestrator !== false
        ? [
            {
              id: "action:open-task-board",
              label: "Open task board",
              group: "Actions",
              onSelect: () => navigate("orchestrator"),
            },
          ]
        : []),
    ];

    return [...routeActions, ...engineActions];
  }, [navVisibility, navigate, toast]);

  usePaletteHotkey(() => {
    if (navigationLock) return;
    setPaletteOpen((v) => !v);
  });

  if (!authed) {
    if (authTransient) {
      return (
        <ReconnectingPage
          controlPanelName={identity.controlPanelName}
          controlPanelMode={controlPanelMode}
          controlPanelModeReason={String(
            capabilitiesQuery.data?.control_panel_mode_reason || ""
          ).trim()}
          errorMessage={authQuery.error?.message || "Engine is reconnecting."}
          onRetry={() => {
            void authQuery.refetch();
          }}
          onCheckEngine={async () => {
            const health = await api("/api/system/health", { cache: "no-store" });
            const status =
              health?.engine?.ready || health?.engine?.healthy ? "healthy" : "unhealthy";
            return `Engine check: ${status} at ${health?.engineUrl || "n/a"}`;
          }}
        />
      );
    }
    return (
      <LoginPage
        loginMutation={loginMutation as any}
        savedToken={getSavedToken()}
        controlPanelName={identity.controlPanelName}
        controlPanelMode={controlPanelMode}
        controlPanelModeReason={String(
          capabilitiesQuery.data?.control_panel_mode_reason || ""
        ).trim()}
        hostedManaged={hostedManaged}
        hostedAuthAvailable={hostedAuthAvailable}
        hostedLoginUrl={String(capabilitiesQuery.data?.hosted_panel_login_url || "")}
        onCheckEngine={async () => {
          const health = await api("/api/system/health");
          const status = health?.engine?.ready || health?.engine?.healthy ? "healthy" : "unhealthy";
          return `Engine check: ${status} at ${health?.engineUrl || "n/a"}`;
        }}
      />
    );
  }

  const providerBadge = providerQuery.data?.ready ? "tcp-badge-ok" : "tcp-badge-warn";
  const providerText = providerQuery.data?.ready
    ? `${providerQuery.data?.defaultProvider || "none"}/${providerQuery.data?.defaultModel || "none"}`
    : "provider setup required";
  const incidentMonitorStatusPayload = (incidentMonitorQuery.data as any) || null;
  const incidentMonitorStatus =
    incidentMonitorStatusPayload?.status || incidentMonitorStatusPayload || null;
  const incidentMonitorEnabled = !!incidentMonitorStatus?.config?.enabled;
  const incidentMonitorPendingIncidents = Number(
    incidentMonitorStatus?.runtime?.pending_incidents || 0
  );
  const incidentMonitorMonitoringActive = !!incidentMonitorStatus?.runtime?.monitoring_active;
  const incidentMonitorPaused = !!incidentMonitorStatus?.runtime?.paused;
  const incidentMonitorIngestReady = !!incidentMonitorStatus?.readiness?.ingest_ready;
  const incidentMonitorPublishReady = !!incidentMonitorStatus?.readiness?.publish_ready;
  const incidentMonitorLastError = String(
    incidentMonitorStatus?.runtime?.last_runtime_error || incidentMonitorStatus?.last_error || ""
  ).trim();

  return (
    <>
      <AppShell
        identity={identity}
        currentRoute={currentRoute}
        providerLocked={providerLocked}
        navRoutes={navRoutes as Array<[string, string, string]>}
        onNavigate={navigate}
        onPaletteOpen={() => setPaletteOpen(true)}
        onThemeCycle={() => setTheme(cycleThemeId(themeId))}
        onLogout={logout}
        navigationLock={navigationLock}
        statusBar={{
          engineHealthy: !!(healthQuery.data?.engine?.ready || healthQuery.data?.engine?.healthy),
          providerBadge,
          providerText,
          activeRuns: ["planning", "awaiting_approval", "running"].includes(
            String((swarmStatusQuery.data as any)?.status || "").toLowerCase()
          )
            ? 1
            : 0,
          incidentMonitor: incidentMonitorEnabled
            ? {
                enabled: true,
                monitoringActive: incidentMonitorMonitoringActive,
                paused: incidentMonitorPaused,
                pendingIncidents: incidentMonitorPendingIncidents,
                blocked: !incidentMonitorIngestReady,
                lastError:
                  incidentMonitorMonitoringActive &&
                  !incidentMonitorPublishReady &&
                  incidentMonitorLastError
                    ? `Watching locally only. ${incidentMonitorLastError}`
                    : incidentMonitorLastError,
              }
            : null,
          approvals: {
            pendingCount: Number(pendingApprovalsQuery.data?.count || 0),
            checking: pendingApprovalsQuery.isFetching,
          },
        }}
        routeKey={currentRoute}
        providerGate={
          providerLocked && currentRoute !== "settings" && !providerNoticeDismissed ? (
            <div className="tcp-panel-card flex flex-wrap items-center justify-between gap-3 border-amber-500/40 px-4 py-3">
              <div className="flex min-w-0 items-center gap-3">
                <i data-lucide="triangle-alert" className="text-amber-300"></i>
                <div className="min-w-0">
                  <div className="text-sm font-semibold">Provider setup required</div>
                  <p className="tcp-subtle text-xs">
                    Configure a provider and default model to unlock all sections.
                  </p>
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <button className="tcp-btn-primary" onClick={() => navigate("settings")}>
                  Open Providers
                </button>
                <button
                  type="button"
                  className="tcp-icon-btn"
                  title="Dismiss"
                  aria-label="Dismiss provider setup notice"
                  onClick={dismissProviderNotice}
                >
                  <i data-lucide="x"></i>
                </button>
              </div>
            </div>
          ) : null
        }
      >
        <HashRouteOutlet routeId={currentRoute} pageProps={commonPageProps} />
      </AppShell>

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        actions={paletteActions}
      />
    </>
  );
}

export function App() {
  return (
    <ToastProvider>
      <AppBody />
    </ToastProvider>
  );
}
