import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { MOTION_TOKENS, prefersReducedMotion } from "./themes.js";
import { renderIcons } from "./icons.js";
import { GlowLayer, IconButton, StatusPulse } from "../ui/index.tsx";

const ROUTE_META: Record<string, { title: string; subtitle: string }> = {
  dashboard: {
    title: "Overview",
    subtitle: "Command status, activity, and fast paths into the system.",
  },
  chat: {
    title: "Chat",
    subtitle: "Session-driven conversation, tools, uploads, and live responses.",
  },
  studio: {
    title: "Studio",
    subtitle: "Template-first workflow builder with reusable role prompts and visual stages.",
  },
  automations: {
    title: "Automations",
    subtitle: "Templates, routines, approvals, and execution history.",
  },
  coding: {
    title: "Coding Workflows",
    subtitle: "Internal Kanban, manual task launchers, and MCP-aware coding runs.",
  },
  agents: {
    title: "Agents",
    subtitle: "Search reusable roles, inspect routines, and manage workflow-ready agent drafts.",
  },
  orchestrator: {
    title: "Orchestrator",
    subtitle: "Plan-driven task execution with workspace visibility and approvals.",
  },
  memory: {
    title: "Memory",
    subtitle: "Searchable memory records and operational context snapshots.",
  },
  feed: {
    title: "Live Feed",
    subtitle: "Global event stream with pack-aware actions and debugging detail.",
  },
  settings: {
    title: "Settings",
    subtitle: "Provider defaults, identity, themes, and runtime diagnostics.",
  },
  channels: {
    title: "Channels",
    subtitle: "Chat integrations, tool scope, and channel configuration.",
  },
  "bug-monitor": {
    title: "Bug Monitor",
    subtitle: "Issue detection, draft review, and GitHub publishing controls.",
  },
  packs: {
    title: "Packs",
    subtitle: "Starter packs and pack installation paths.",
  },
  teams: {
    title: "Teams",
    subtitle: "Team instances, approvals, and shared execution state.",
  },
  mcp: {
    title: "MCP",
    subtitle: "Catalog, readiness, and generated integration details.",
  },
  files: {
    title: "Files",
    subtitle: "Uploaded assets and workspace-adjacent file management.",
  },
  "packs-detail": {
    title: "Packs",
    subtitle: "Starter packs, installation paths, and detected attachments.",
  },
  "teams-detail": {
    title: "Teams",
    subtitle: "Team instances, approvals, and shared execution state.",
  },
};

export function AppShell({
  identity,
  currentRoute,
  providerLocked,
  navRoutes,
  onNavigate,
  onPaletteOpen,
  onThemeCycle,
  onLogout,
  statusBar,
  routeKey,
  children,
  providerGate,
}: {
  identity: { botName: string; botAvatarUrl: string; controlPanelName?: string };
  currentRoute: string;
  providerLocked: boolean;
  navRoutes: Array<[string, string, string]>;
  onNavigate: (route: string) => void;
  onPaletteOpen: () => void;
  onThemeCycle: () => void;
  onLogout: () => void;
  statusBar: {
    engineHealthy: boolean;
    providerBadge: string;
    providerText: string;
    activeRuns: number;
    bugMonitor?: {
      enabled: boolean;
      monitoringActive: boolean;
      paused: boolean;
      pendingIncidents: number;
      blocked: boolean;
      lastError?: string;
    } | null;
  };
  routeKey: string;
  children: any;
  providerGate?: any;
}) {
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [avatarErrored, setAvatarErrored] = useState(false);
  const avatarUrl = String(identity.botAvatarUrl || "").trim();
  const reducedMotion = prefersReducedMotion();

  useEffect(() => {
    setMobileNavOpen(false);
  }, [currentRoute]);

  useEffect(() => {
    try {
      renderIcons();
    } catch {}
  }, [
    currentRoute,
    mobileNavOpen,
    statusBar.bugMonitor?.enabled,
    statusBar.bugMonitor?.monitoringActive,
    statusBar.bugMonitor?.paused,
    statusBar.bugMonitor?.pendingIncidents,
    statusBar.bugMonitor?.blocked,
  ]);

  useEffect(() => {
    setAvatarErrored(false);
  }, [avatarUrl]);

  const routeMeta = ROUTE_META[currentRoute] || {
    title: String(navRoutes.find(([id]) => id === currentRoute)?.[1] || "Control Panel"),
    subtitle: "Desktop-inspired operations UI for Tandem.",
  };

  const currentNav = useMemo(
    () => navRoutes.find(([id]) => id === currentRoute) || navRoutes[0],
    [currentRoute, navRoutes]
  );
  const bugMonitorState = useMemo(() => {
    const monitor = statusBar.bugMonitor;
    if (!monitor?.enabled) return null;
    if (monitor.blocked) {
      return {
        toneClass: "blocked",
        label: "Bug Monitor blocked",
        shortLabel: "Blocked",
      };
    }
    if (monitor.paused) {
      return {
        toneClass: "paused",
        label: "Bug Monitor paused",
        shortLabel: "Paused",
      };
    }
    if (monitor.pendingIncidents > 0) {
      return {
        toneClass: "incidents",
        label: `Bug Monitor incidents: ${monitor.pendingIncidents}`,
        shortLabel: `${monitor.pendingIncidents} incident${monitor.pendingIncidents === 1 ? "" : "s"}`,
      };
    }
    if (monitor.monitoringActive) {
      return {
        toneClass: "watching",
        label: "Bug Monitor watching",
        shortLabel: "Watching",
      };
    }
    return {
      toneClass: "ready",
      label: "Bug Monitor ready",
      shortLabel: "Ready",
    };
  }, [statusBar.bugMonitor]);

  const renderAvatar = () =>
    avatarUrl && !avatarErrored ? (
      <img
        src={avatarUrl}
        alt={identity.botName}
        className="block h-full w-full object-cover"
        onError={() => setAvatarErrored(true)}
      />
    ) : (
      <span className="text-sm font-semibold uppercase">
        {String(identity.botName || "T")
          .trim()
          .slice(0, 1) || "T"}
      </span>
    );

  const renderIconRailItems = () =>
    navRoutes.map(([id, label, icon]) => {
      const active = currentRoute === id;
      const locked = providerLocked && id !== "settings";
      return (
        <button
          key={id}
          type="button"
          title={label}
          disabled={locked}
          className={`tcp-rail-icon ${active ? "active" : ""} ${locked ? "locked" : ""}`}
          onClick={() => onNavigate(id)}
        >
          {active ? (
            <motion.span layoutId="tcp-icon-indicator" className="tcp-rail-icon-indicator" />
          ) : null}
          <i data-lucide={icon}></i>
        </button>
      );
    });

  const renderContextNav = (mobile = false) =>
    navRoutes.map(([id, label, icon]) => {
      const active = currentRoute === id;
      const locked = providerLocked && id !== "settings";
      return (
        <button
          key={id}
          type="button"
          disabled={locked}
          className={`tcp-context-link ${active ? "active" : ""} ${locked ? "locked" : ""}`}
          onClick={() => {
            onNavigate(id);
            if (mobile) setMobileNavOpen(false);
          }}
        >
          <span className="inline-flex items-center gap-2">
            <i data-lucide={icon}></i>
            <span>{label}</span>
          </span>
          {active ? <span className="tcp-context-link-dot"></span> : null}
        </button>
      );
    });

  const contextRail = (mobile = false) => (
    <>
      {mobile ? (
        <div className="tcp-context-hero">
          <GlowLayer className="tcp-context-hero-glow" />
          <div className="relative z-10 flex items-center gap-3">
            <div className="tcp-brand-avatar h-11 w-11">{renderAvatar()}</div>
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold">
                {identity.controlPanelName || `${identity.botName} Control Panel`}
              </div>
              <div className="tcp-subtle text-xs">Workspace navigation and system status</div>
            </div>
          </div>
        </div>
      ) : null}

      <div className={`tcp-context-section ${mobile ? "" : "xl:hidden"}`.trim()}>
        <div className="tcp-context-section-label">Navigation</div>
        <nav className="grid gap-1">{renderContextNav(mobile)}</nav>
      </div>

      {mobile ? (
        <div className="tcp-context-section">
          <div className="tcp-context-section-label">System</div>
          <div className="grid gap-2">
            <div className="tcp-context-stat">
              <span className="tcp-subtle text-xs">Engine</span>
              {statusBar.engineHealthy ? (
                <StatusPulse tone="ok" text="healthy" />
              ) : (
                <StatusPulse tone="warn" text="checking" />
              )}
            </div>
            <div className="tcp-context-stat">
              <span className="tcp-subtle text-xs">Provider</span>
              <span className={statusBar.providerBadge}>{statusBar.providerText}</span>
            </div>
            <div className="tcp-context-stat">
              <span className="tcp-subtle text-xs">Active runs</span>
              {statusBar.activeRuns > 0 ? (
                <StatusPulse tone="live" text={String(statusBar.activeRuns)} />
              ) : (
                <span className="tcp-badge tcp-badge-ghost">idle</span>
              )}
            </div>
            {bugMonitorState ? (
              <div className="tcp-context-stat">
                <span className="tcp-subtle text-xs">Bug Monitor</span>
                <button
                  type="button"
                  className={`tcp-bug-monitor-pill ${bugMonitorState.toneClass}`}
                  title={
                    statusBar.bugMonitor?.lastError
                      ? `${bugMonitorState.label}: ${statusBar.bugMonitor.lastError}`
                      : bugMonitorState.label
                  }
                  onClick={() => {
                    onNavigate("bug-monitor");
                    if (mobile) setMobileNavOpen(false);
                  }}
                >
                  <i data-lucide="bug-play"></i>
                  <span className="tcp-bug-monitor-dot" aria-hidden="true"></span>
                  <span>{bugMonitorState.shortLabel}</span>
                </button>
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      <div className="tcp-context-section mt-auto">
        <div className="tcp-context-section-label">Actions</div>
        <div className="grid gap-2">
          <button
            type="button"
            className="tcp-btn w-full justify-start"
            onClick={() => {
              onPaletteOpen();
              if (mobile) setMobileNavOpen(false);
            }}
          >
            <i data-lucide="search"></i>
            Command palette
          </button>
          <button
            type="button"
            className="tcp-btn w-full justify-start"
            onClick={() => {
              onThemeCycle();
              if (mobile) setMobileNavOpen(false);
            }}
          >
            <i data-lucide="paint-bucket"></i>
            Cycle theme
          </button>
          <button
            type="button"
            className="tcp-btn w-full justify-start"
            onClick={() => {
              onLogout();
              if (mobile) setMobileNavOpen(false);
            }}
          >
            <i data-lucide="log-out"></i>
            Logout
          </button>
        </div>
      </div>
    </>
  );

  return (
    <div className={`tcp-shell ${currentRoute === "chat" ? "tcp-shell-chat" : ""}`.trim()}>
      <GlowLayer className="tcp-shell-background">
        <div className="tcp-shell-glow tcp-shell-glow-a"></div>
        <div className="tcp-shell-glow tcp-shell-glow-b"></div>
      </GlowLayer>

      <aside className="tcp-icon-rail hidden xl:flex">
        <button type="button" className="tcp-rail-brand" onClick={() => onNavigate("dashboard")}>
          <div className="tcp-brand-avatar h-10 w-10">{renderAvatar()}</div>
        </button>
        <nav className="tcp-rail-nav">{renderIconRailItems()}</nav>
        <div className="tcp-rail-footer">
          <IconButton title="Command palette" onClick={onPaletteOpen}>
            <i data-lucide="search"></i>
          </IconButton>
          <IconButton title="Cycle theme" onClick={onThemeCycle}>
            <i data-lucide="paint-bucket"></i>
          </IconButton>
          <IconButton title="Logout" onClick={onLogout}>
            <i data-lucide="log-out"></i>
          </IconButton>
          <div className="mt-2 flex justify-center">
            {statusBar.engineHealthy ? <StatusPulse tone="ok" /> : <StatusPulse tone="warn" />}
          </div>
        </div>
      </aside>

      <aside className="tcp-context-rail hidden lg:flex xl:hidden">{contextRail(false)}</aside>

      <main
        className={`tcp-main-shell ${currentRoute === "chat" ? "tcp-main-shell-fill" : ""}`.trim()}
      >
        <section className="tcp-mobile-topbar lg:hidden">
          <button
            type="button"
            className="tcp-btn h-10 px-3"
            onClick={() => setMobileNavOpen(true)}
          >
            <i data-lucide="menu"></i>
            Menu
          </button>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">{routeMeta.title}</div>
            <div className="tcp-subtle truncate text-xs">
              {currentNav?.[1] || routeMeta.subtitle}
            </div>
          </div>
          {bugMonitorState ? (
            <button
              type="button"
              className={`tcp-bug-monitor-pill ${bugMonitorState.toneClass}`}
              title={
                statusBar.bugMonitor?.lastError
                  ? `${bugMonitorState.label}: ${statusBar.bugMonitor.lastError}`
                  : bugMonitorState.label
              }
              onClick={() => onNavigate("bug-monitor")}
            >
              <i data-lucide="bug-play"></i>
              <span className="tcp-bug-monitor-dot" aria-hidden="true"></span>
            </button>
          ) : null}
          {statusBar.activeRuns > 0 ? (
            <StatusPulse tone="live" text={String(statusBar.activeRuns)} />
          ) : null}
        </section>

        <section className="tcp-topbar">
          <div className="min-w-0">
            <div className="tcp-page-eyebrow">Tandem Control</div>
            <h1 className="tcp-main-title">{routeMeta.title}</h1>
            <p className="tcp-subtle mt-1 max-w-2xl">{routeMeta.subtitle}</p>
          </div>
          <div className="tcp-topbar-status">
            {bugMonitorState ? (
              <button
                type="button"
                className={`tcp-bug-monitor-pill ${bugMonitorState.toneClass}`}
                title={
                  statusBar.bugMonitor?.lastError
                    ? `${bugMonitorState.label}: ${statusBar.bugMonitor.lastError}`
                    : bugMonitorState.label
                }
                onClick={() => onNavigate("bug-monitor")}
              >
                <i data-lucide="bug-play"></i>
                <span className="tcp-bug-monitor-dot" aria-hidden="true"></span>
                <span>{bugMonitorState.shortLabel}</span>
              </button>
            ) : null}
            <span className={statusBar.providerBadge}>{statusBar.providerText}</span>
            {statusBar.engineHealthy ? (
              <StatusPulse tone="ok" text="Engine healthy" />
            ) : (
              <StatusPulse tone="warn" text="Checking engine" />
            )}
            {statusBar.activeRuns > 0 ? (
              <StatusPulse tone="live" text={`${statusBar.activeRuns} run`} />
            ) : (
              <span className="tcp-badge tcp-badge-ghost">No active runs</span>
            )}
          </div>
        </section>

        <AnimatePresence mode="wait">
          <motion.section
            key={routeKey}
            className={`tcp-main-content ${currentRoute === "chat" ? "tcp-main-content-fill" : ""}`.trim()}
            initial={reducedMotion ? false : { opacity: 0, y: 18 }}
            animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
            exit={reducedMotion ? undefined : { opacity: 0, y: -14 }}
            transition={
              reducedMotion
                ? undefined
                : {
                    duration: MOTION_TOKENS.duration.normal,
                    ease: MOTION_TOKENS.easing.standard,
                  }
            }
          >
            {children}
          </motion.section>
        </AnimatePresence>
      </main>

      <AnimatePresence>
        {mobileNavOpen ? (
          <motion.div
            className="tcp-mobile-drawer lg:hidden"
            initial={reducedMotion ? false : { opacity: 0 }}
            animate={reducedMotion ? undefined : { opacity: 1 }}
            exit={reducedMotion ? undefined : { opacity: 0 }}
          >
            <button
              type="button"
              className="tcp-mobile-drawer-backdrop"
              aria-label="Close navigation"
              onClick={() => setMobileNavOpen(false)}
            />
            <motion.aside
              className="tcp-mobile-drawer-panel"
              initial={reducedMotion ? false : { x: "-100%" }}
              animate={reducedMotion ? undefined : { x: 0 }}
              exit={reducedMotion ? undefined : { x: "-100%" }}
              transition={reducedMotion ? undefined : MOTION_TOKENS.spring.drawer}
            >
              <div className="mb-3 flex items-center justify-between">
                <div>
                  <div className="text-sm font-semibold">{identity.botName}</div>
                  <div className="tcp-subtle text-xs">{routeMeta.title}</div>
                </div>
                <IconButton title="Close" onClick={() => setMobileNavOpen(false)}>
                  <i data-lucide="x"></i>
                </IconButton>
              </div>
              {contextRail(true)}
            </motion.aside>
          </motion.div>
        ) : null}
      </AnimatePresence>

      <AnimatePresence>{providerGate || null}</AnimatePresence>
    </div>
  );
}
