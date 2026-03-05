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
import { providerHints } from "./store.js";
import { THEMES, applyTheme, getActiveThemeId, setControlPanelTheme } from "./themes.js";
import { renderIcons } from "./icons.js";
import { api } from "../lib/api";
import { useSwarmStatus, useSystemHealth } from "../features/system/queries";

const TOKEN_STORAGE_KEY = "tandem_control_panel_token";

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

type ProviderState = {
  ready: boolean;
  defaultProvider: string;
  defaultModel: string;
  connected: string[];
  error: string;
  needsOnboarding: boolean;
};

function useProviderStatus(client: TandemClient | null, enabled: boolean) {
  return useQuery<ProviderState>({
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
        };
      }
      try {
        const [config, catalog, authStatus] = await Promise.all([
          client.providers.config(),
          client.providers.catalog(),
          client.providers.authStatus().catch(() => ({})),
        ]);
        const defaultProvider = String(config?.default || "").trim();
        const defaultModel = String(
          config?.providers?.[defaultProvider]?.default_model || ""
        ).trim();
        const connected = new Set(
          (catalog?.connected || []).map((id: string) =>
            String(id || "")
              .trim()
              .toLowerCase()
          )
        );
        const providerNeedsApiKey = (providerId: string) => {
          const id = String(providerId || "")
            .trim()
            .toLowerCase();
          return !!id && id !== "ollama" && id !== "local";
        };
        const hasStoredKey = (() => {
          const id = String(defaultProvider || "")
            .trim()
            .toLowerCase();
          if (!id) return false;
          if (authStatus && typeof authStatus === "object") {
            const direct = (authStatus as any)[id];
            if (direct && typeof direct === "object") {
              if (direct.has_key === true || direct.hasKey === true) return true;
              if (direct.configured === true && !providerNeedsApiKey(id)) return true;
            }
            const nested = (authStatus as any).providers?.[id];
            if (nested && typeof nested === "object") {
              if (nested.has_key === true || nested.hasKey === true) return true;
              if (nested.configured === true && !providerNeedsApiKey(id)) return true;
            }
          }
          return false;
        })();
        const ready =
          !!defaultProvider &&
          !!defaultModel &&
          connected.has(
            String(defaultProvider || "")
              .trim()
              .toLowerCase()
          ) &&
          (!providerNeedsApiKey(defaultProvider) || hasStoredKey);

        return {
          ready,
          defaultProvider,
          defaultModel,
          connected: [...connected],
          error: "",
          needsOnboarding: !ready,
        };
      } catch (error) {
        return {
          ready: false,
          defaultProvider: "",
          defaultModel: "",
          connected: [],
          error: error instanceof Error ? error.message : String(error),
          needsOnboarding: true,
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
        const payload = client?.identity?.get
          ? await client.identity.get()
          : await api("/api/engine/config/identity", { method: "GET" });
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

function AppBody() {
  const queryClient = useQueryClient();
  const { toast } = useToast();
  const { route, navigate } = useHashRoute();
  const [themeId, setThemeId] = useState(getActiveThemeId());
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [providerGateNoticeShown, setProviderGateNoticeShown] = useState(false);
  const autoLoginAttempted = useRef(false);

  useEffect(() => {
    applyTheme(themeId);
  }, [themeId]);

  const authQuery = useQuery({
    queryKey: ["auth", "me"],
    retry: false,
    refetchInterval: 30000,
    queryFn: () => api("/api/auth/me", { method: "GET" }),
  });

  const authed = authQuery.isSuccess;
  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  }, [authed, route]);
  const client = useMemo(
    () => (authed ? new TandemClient({ baseUrl: "/api/engine", token: "session" }) : null),
    [authed]
  );

  const providerQuery = useProviderStatus(client, authed);
  const identityQuery = useIdentity(client, authed);
  const healthQuery = useSystemHealth(authed);
  const swarmStatusQuery = useSwarmStatus(authed);

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
      toast("err", error instanceof Error ? error.message : String(error));
    },
  });

  useEffect(() => {
    if (authed || loginMutation.isPending || autoLoginAttempted.current) return;
    const savedToken = getSavedToken().trim();
    if (!savedToken) {
      autoLoginAttempted.current = true;
      return;
    }
    autoLoginAttempted.current = true;
    loginMutation.mutate({ token: savedToken, remember: true });
  }, [authed, loginMutation]);

  const logout = useCallback(async () => {
    await api("/api/auth/logout", { method: "POST" }).catch(() => {});
    queryClient.removeQueries({ queryKey: ["auth"] });
    queryClient.removeQueries({ queryKey: ["provider"] });
    queryClient.removeQueries({ queryKey: ["identity"] });
    queryClient.invalidateQueries({ queryKey: ["auth", "me"] });
    toast("info", "Logged out.");
  }, [queryClient, toast]);

  const lockedRoutes = useMemo(() => new Set(["chat", "agents", "orchestrator", "teams"]), []);
  const needsProviderOnboarding = !!providerQuery.data?.needsOnboarding;
  const providerLocked = authed && needsProviderOnboarding;

  useEffect(() => {
    if (!providerLocked) {
      setProviderGateNoticeShown(false);
      return;
    }
    if (!lockedRoutes.has(route)) return;
    navigate("settings");
    if (!providerGateNoticeShown) {
      setProviderGateNoticeShown(true);
      toast("info", "Set provider + default model first to unlock the control panel.");
    }
  }, [lockedRoutes, navigate, providerGateNoticeShown, providerLocked, route, toast]);

  const currentRoute = providerLocked && lockedRoutes.has(route) ? "settings" : route;

  const refreshProviderStatus = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["provider", "status"] });
  }, [queryClient]);

  const refreshIdentityStatus = useCallback(async () => {
    await queryClient.invalidateQueries({ queryKey: ["identity"] });
  }, [queryClient]);

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
    },
    identity,
    refreshProviderStatus,
    refreshIdentityStatus,
    providerHints,
    themes: THEMES,
    setTheme,
    themeId,
  };

  const paletteActions = useMemo<PaletteAction[]>(() => {
    const routeActions = APP_ROUTES.map(([id, label]) => ({
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
            toast("err", error instanceof Error ? error.message : String(error));
          }
        },
      },
      {
        id: "action:open-settings",
        label: "Open provider settings",
        group: "Actions",
        onSelect: () => navigate("settings"),
      },
      {
        id: "action:open-orchestrator",
        label: "Open orchestrator",
        group: "Actions",
        onSelect: () => navigate("orchestrator"),
      },
    ];

    return [...routeActions, ...engineActions];
  }, [navigate, toast]);

  usePaletteHotkey(() => setPaletteOpen((v) => !v));

  if (!authed) {
    return (
      <LoginPage
        loginMutation={loginMutation as any}
        savedToken={getSavedToken()}
        controlPanelName={identity.controlPanelName}
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

  return (
    <>
      <AppShell
        identity={identity}
        currentRoute={currentRoute}
        providerLocked={providerLocked}
        navRoutes={APP_NAV_ROUTES}
        onNavigate={navigate}
        onPaletteOpen={() => setPaletteOpen(true)}
        onThemeCycle={() =>
          setTheme(THEMES[(THEMES.findIndex((t) => t.id === themeId) + 1) % THEMES.length].id)
        }
        onLogout={logout}
        statusBar={{
          engineHealthy: !!(healthQuery.data?.engine?.ready || healthQuery.data?.engine?.healthy),
          providerBadge,
          providerText,
          activeRuns: ["planning", "awaiting_approval", "running"].includes(
            String((swarmStatusQuery.data as any)?.status || "").toLowerCase()
          )
            ? 1
            : 0,
        }}
        routeKey={currentRoute}
        providerGate={
          providerLocked && currentRoute !== "settings" ? (
            <motion.div
              className="tcp-confirm-overlay"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
            >
              <motion.div
                className="tcp-confirm-dialog"
                initial={{ opacity: 0, y: 8, scale: 0.98 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: 6, scale: 0.98 }}
              >
                <h3 className="tcp-confirm-title">Provider Setup Required</h3>
                <p className="tcp-confirm-message">
                  Configure provider and default model in Settings to unlock all sections.
                </p>
                <div className="tcp-confirm-actions">
                  <button className="tcp-btn-primary" onClick={() => navigate("settings")}>
                    Open Settings
                  </button>
                </div>
              </motion.div>
            </motion.div>
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
