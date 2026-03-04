import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { renderIcons } from "../app/icons.js";
import { escapeHtml } from "../app/dom.js";
import type { RouteId } from "../app/routes";

export type LegacyPageProps = {
  routeId: RouteId;
  renderer: (ctx: any) => Promise<void> | void;
  legacyState: any;
  api: (path: string, init?: RequestInit) => Promise<any>;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
  navigate: (route: string) => void;
  refreshProviderStatus: () => Promise<void>;
  refreshIdentityStatus: () => Promise<void>;
  renderShell: () => void;
  providerHints: Record<string, unknown>;
  routes: any[];
  themes: any[];
  setTheme: (themeId: string) => any;
};

function buildScopedById(root: HTMLElement) {
  return (id: string) => {
    if (!id) return null;
    const escaped = typeof CSS !== "undefined" && CSS.escape ? CSS.escape(id) : id;
    const local = root.querySelector<HTMLElement>(`#${escaped}`);
    if (local) return local;
    return document.getElementById(id);
  };
}

export function LegacyPage(props: LegacyPageProps) {
  const {
    routeId,
    renderer,
    legacyState,
    api,
    toast,
    navigate,
    refreshProviderStatus,
    refreshIdentityStatus,
    renderShell,
    providerHints,
    routes,
    themes,
    setTheme,
  } = props;
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [loading, setLoading] = useState(true);
  const renderToken = useRef(0);

  const ctx = useMemo(
    () => ({
      app: hostRef.current,
      state: legacyState,
      api,
      byId: hostRef.current
        ? buildScopedById(hostRef.current)
        : (id: string) => document.getElementById(id),
      escapeHtml,
      ROUTES: routes,
      providerHints,
      toast,
      addCleanup: (fn: () => void) => {
        if (!Array.isArray(legacyState.cleanup)) legacyState.cleanup = [];
        legacyState.cleanup.push(fn);
      },
      clearCleanup: () => {
        for (const fn of legacyState.cleanup || []) {
          try {
            fn();
          } catch {
            // ignore cleanup failures
          }
        }
        legacyState.cleanup = [];
      },
      setRoute: navigate,
      renderShell,
      refreshProviderStatus,
      refreshIdentityStatus,
      renderIcons,
      THEMES: themes,
      setTheme,
    }),
    [
      api,
      legacyState,
      navigate,
      providerHints,
      refreshIdentityStatus,
      refreshProviderStatus,
      renderShell,
      routes,
      setTheme,
      themes,
      toast,
    ]
  );

  useEffect(() => {
    legacyState.route = routeId;
    legacyState.cleanup = [];
    const token = ++renderToken.current;
    setLoading(true);

    const root = hostRef.current;
    if (!root) return;

    root.innerHTML = '<section id="view" class="grid h-full gap-4 tcp-view-surface"></section>';

    Promise.resolve(renderer(ctx))
      .catch((error) => {
        const view = root.querySelector<HTMLElement>("#view");
        if (view) {
          view.innerHTML = `
            <div class="tcp-card">
              <h3 class="tcp-title">View Error</h3>
              <p class="tcp-subtle mt-2">${escapeHtml(error?.message || String(error || "Unknown error"))}</p>
            </div>
          `;
        }
      })
      .finally(() => {
        if (renderToken.current !== token) return;
        renderIcons(root);
        setLoading(false);
      });

    return () => {
      for (const fn of legacyState.cleanup || []) {
        try {
          fn();
        } catch {
          // ignore cleanup failures
        }
      }
      legacyState.cleanup = [];
    };
  }, [ctx, legacyState, renderer, routeId]);

  return (
    <motion.div
      key={routeId}
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -4 }}
      transition={{ duration: 0.16, ease: "easeOut" }}
      className="relative h-full"
    >
      <AnimatePresence>
        {loading ? (
          <motion.div
            key="loading"
            className="tcp-loading-shell"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <div className="tcp-skeleton h-6 w-44" />
            <div className="tcp-skeleton h-24 w-full" />
            <div className="grid gap-3 md:grid-cols-2">
              <div className="tcp-skeleton h-32 w-full" />
              <div className="tcp-skeleton h-32 w-full" />
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
      <div ref={hostRef} className={loading ? "opacity-0" : "opacity-100 transition-opacity"} />
    </motion.div>
  );
}
