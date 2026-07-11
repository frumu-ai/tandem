import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";
import { MOTION_TOKENS, prefersReducedMotion } from "./themes.js";
import { groupNavRoutes } from "./store.js";
import { IconButton, StatusPulse } from "../ui/index.tsx";
import { TandemLogoAnimation } from "../ui/TandemLogoAnimation";
import type { NavigationLockState } from "../pages/pageTypes";
import { Icon, type IconName } from "../ui/Icon";

// Keep subtitles to a short descriptor — the shell header shows one line and the
// page body carries the detail. Every primary nav route needs an entry so none
// falls back to the generic "Control Panel" placeholder (see TAN-585).
const ROUTE_META: Record<string, { title: string; subtitle: string }> = {
  dashboard: { title: "Overview", subtitle: "Status and activity" },
  chat: { title: "Chat", subtitle: "Sessions, tools, and uploads" },
  planner: { title: "Planner", subtitle: "Long-horizon multi-agent planning" },
  workflows: { title: "Workflows", subtitle: "Build and run workflows" },
  marketplace: { title: "Marketplace", subtitle: "Templates and starter packs" },
  studio: { title: "Studio", subtitle: "Template-first workflow builder" },
  automations: { title: "Automations", subtitle: "Schedules, library, and run history" },
  webhooks: { title: "Webhooks", subtitle: "External triggers and deliveries" },
  experiments: { title: "Experiments", subtitle: "Experimental surfaces" },
  "enterprise-admin": { title: "Enterprise", subtitle: "Org units, access, and connectors" },
  coding: { title: "Coder", subtitle: "Durable coder runs and repos" },
  agents: { title: "Agents", subtitle: "Reusable roles and drafts" },
  orchestrator: { title: "Task Board", subtitle: "Plan-driven task execution" },
  memory: { title: "Memory", subtitle: "Records and context snapshots" },
  runs: { title: "Runs", subtitle: "Queue state and run inspection" },
  "control-loop": { title: "Control Loop", subtitle: "Goal-to-audit traceability" },
  approvals: { title: "Approvals Inbox", subtitle: "Pending human approvals" },
  settings: { title: "Settings", subtitle: "Providers, identity, and diagnostics" },
  channels: { title: "Channels", subtitle: "Chat integrations and scope" },
  "incident-monitor": { title: "Incident Monitor", subtitle: "Detect, review, and publish" },
  packs: { title: "Packs", subtitle: "Starter packs" },
  teams: { title: "Teams", subtitle: "Team instances and shared state" },
  mcp: { title: "MCP", subtitle: "Catalog and readiness" },
  files: { title: "Files", subtitle: "Uploads, artifacts, and exports" },
  "packs-detail": { title: "Packs", subtitle: "Starter packs" },
  "teams-detail": { title: "Teams", subtitle: "Team instances and shared state" },
};

const FULL_HEIGHT_ROUTES = new Set([
  "chat",
  "automations",
  "webhooks",
  "approvals",
  "files",
  "runs",
  "control-loop",
]);

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
  navigationLock,
  children,
  providerGate,
}: {
  identity: { botName: string; botAvatarUrl: string; controlPanelName?: string };
  currentRoute: string;
  providerLocked: boolean;
  navRoutes: Array<[string, string, IconName]>;
  onNavigate: (route: string) => void;
  onPaletteOpen: () => void;
  onThemeCycle: () => void;
  onLogout: () => void;
  statusBar: {
    engineHealthy: boolean;
    providerBadge: string;
    providerText: string;
    activeRuns: number;
    incidentMonitor?: {
      enabled: boolean;
      monitoringActive: boolean;
      paused: boolean;
      pendingIncidents: number;
      blocked: boolean;
      lastError?: string;
    } | null;
    approvals?: {
      pendingCount: number;
      checking?: boolean;
    } | null;
  };
  routeKey: string;
  navigationLock?: NavigationLockState | null;
  children: any;
  providerGate?: any;
}) {
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const avatarUrl = String(identity.botAvatarUrl || "").trim();
  const [avatarMode, setAvatarMode] = useState<"custom" | "default" | "fallback">(
    avatarUrl ? "custom" : "default"
  );
  const defaultAvatarUrl = "/icon.png";
  const reducedMotion = prefersReducedMotion();
  const navigationLocked = !!navigationLock;

  useEffect(() => {
    setMobileNavOpen(false);
  }, [currentRoute]);

  useEffect(() => {
    setAvatarMode(avatarUrl ? "custom" : "default");
  }, [avatarUrl]);

  const routeMeta = ROUTE_META[currentRoute] || {
    title: String(navRoutes.find(([id]) => id === currentRoute)?.[1] || "Control Panel"),
    subtitle: "Desktop-inspired operations UI for Tandem.",
  };

  const currentNav = useMemo(
    () => navRoutes.find(([id]) => id === currentRoute) || null,
    [currentRoute, navRoutes]
  );
  const incidentMonitorState = useMemo(() => {
    const monitor = statusBar.incidentMonitor;
    if (!monitor?.enabled) return null;
    if (monitor.blocked) {
      return {
        toneClass: "blocked",
        label: "Incident Monitor blocked",
        shortLabel: "Blocked",
      };
    }
    if (monitor.paused) {
      return {
        toneClass: "paused",
        label: "Incident Monitor paused",
        shortLabel: "Paused",
      };
    }
    if (monitor.pendingIncidents > 0) {
      return {
        toneClass: "incidents",
        label: `Incident Monitor incidents: ${monitor.pendingIncidents}`,
        shortLabel: `${monitor.pendingIncidents} incident${monitor.pendingIncidents === 1 ? "" : "s"}`,
      };
    }
    if (monitor.monitoringActive) {
      return {
        toneClass: "watching",
        label: "Incident Monitor watching",
        shortLabel: "Watching",
      };
    }
    return {
      toneClass: "ready",
      label: "Incident Monitor ready",
      shortLabel: "Ready",
    };
  }, [statusBar.incidentMonitor]);
  const pendingApprovalCount = Math.max(0, Number(statusBar.approvals?.pendingCount || 0));
  const approvalLabel = `${pendingApprovalCount} approval${
    pendingApprovalCount === 1 ? "" : "s"
  } pending`;

  const renderAvatar = () =>
    avatarMode !== "fallback" ? (
      <img
        src={avatarMode === "custom" ? avatarUrl : defaultAvatarUrl}
        alt={identity.botName || "Tandem"}
        className="block h-full w-full object-contain p-0.5"
        onError={() => setAvatarMode((current) => (current === "custom" ? "default" : "fallback"))}
      />
    ) : (
      <span className="text-sm font-semibold uppercase">
        {String(identity.botName || "T")
          .trim()
          .slice(0, 1) || "T"}
      </span>
    );

  const renderIconRailItems = () =>
    groupNavRoutes(navRoutes).flatMap((group, groupIndex) => {
      const buttons = group.items.map(([id, label, icon]) => {
        const active = currentRoute === id;
        const locked = providerLocked && id !== "settings";
        const disabled = locked || navigationLocked;
        return (
          <button
            key={id}
            type="button"
            title={label}
            aria-label={label}
            disabled={disabled}
            className={`tcp-rail-icon ${active ? "active" : ""} ${disabled ? "locked" : ""}`}
            onClick={() => onNavigate(id)}
          >
            {active ? (
              <motion.span layoutId="tcp-icon-indicator" className="tcp-rail-icon-indicator" />
            ) : null}
            <Icon name={icon} />
          </button>
        );
      });
      return groupIndex > 0
        ? [
            <span key={`rail-divider-${group.label}`} className="tcp-rail-divider" aria-hidden="true" />,
            ...buttons,
          ]
        : buttons;
    });

  const renderContextNavButton = ([id, label, icon]: [string, string, IconName], mobile: boolean) => {
    const active = currentRoute === id;
    const locked = providerLocked && id !== "settings";
    const disabled = locked || navigationLocked;
    return (
      <button
        key={id}
        type="button"
        disabled={disabled}
        className={`tcp-context-link ${active ? "active" : ""} ${disabled ? "locked" : ""}`}
        onClick={() => {
          onNavigate(id);
          if (mobile) setMobileNavOpen(false);
        }}
      >
        <span className="inline-flex items-center gap-2">
          <Icon name={icon} />
          <span>{label}</span>
        </span>
        {active ? <span className="tcp-context-link-dot"></span> : null}
      </button>
    );
  };

  const renderContextNav = (mobile = false) =>
    groupNavRoutes(navRoutes).map((group) => (
      <div key={group.label} className="grid gap-1">
        <div className="tcp-context-section-label px-1">{group.label}</div>
        {group.items.map((route) => renderContextNavButton(route, mobile))}
      </div>
    ));

  const contextRail = (mobile = false) => (
    <>
      {mobile ? (
        <div className="tcp-context-hero">
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
        <nav className="grid gap-4">{renderContextNav(mobile)}</nav>
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
            {incidentMonitorState ? (
              <div className="tcp-context-stat">
                <span className="tcp-subtle text-xs">Incident Monitor</span>
                <button
                  type="button"
                  className={`tcp-incident-monitor-pill ${incidentMonitorState.toneClass}`}
                  disabled={navigationLocked}
                  title={
                    statusBar.incidentMonitor?.lastError
                      ? `${incidentMonitorState.label}: ${statusBar.incidentMonitor.lastError}`
                      : incidentMonitorState.label
                  }
                  onClick={() => {
                    onNavigate("incident-monitor");
                    if (mobile) setMobileNavOpen(false);
                  }}
                >
                  <Icon name="shield-alert" />
                  <span className="tcp-incident-monitor-dot" aria-hidden="true"></span>
                  <span>{incidentMonitorState.shortLabel}</span>
                </button>
              </div>
            ) : null}
            {pendingApprovalCount > 0 ? (
              <div className="tcp-context-stat">
                <span className="tcp-subtle text-xs">Approvals</span>
                <button
                  type="button"
                  className="tcp-approval-pill"
                  disabled={navigationLocked}
                  title={approvalLabel}
                  onClick={() => {
                    onNavigate("approvals");
                    if (mobile) setMobileNavOpen(false);
                  }}
                >
                  <Icon name="shield-alert" />
                  <span>{pendingApprovalCount}</span>
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
            <Icon name="search" />
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
            <Icon name="paint-bucket" />
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
            <Icon name="log-out" />
            Logout
          </button>
        </div>
      </div>
    </>
  );

  return (
    <div className={`tcp-shell ${currentRoute === "chat" ? "tcp-shell-chat" : ""}`.trim()}>
      <aside className="tcp-icon-rail hidden xl:flex">
        <button
          type="button"
          className="tcp-rail-brand"
          disabled={navigationLocked}
          onClick={() => onNavigate("dashboard")}
        >
          <div className="tcp-brand-avatar h-10 w-10">{renderAvatar()}</div>
        </button>
        <nav className="tcp-rail-nav">{renderIconRailItems()}</nav>
        <div className="tcp-rail-footer">
          <IconButton aria-label="Command palette" onClick={onPaletteOpen} disabled={navigationLocked}>
            <Icon name="search" />
          </IconButton>
          <IconButton aria-label="Cycle theme" onClick={onThemeCycle} disabled={navigationLocked}>
            <Icon name="paint-bucket" />
          </IconButton>
          <IconButton aria-label="Logout" onClick={onLogout} disabled={navigationLocked}>
            <Icon name="log-out" />
          </IconButton>
          <div className="mt-2 flex justify-center">
            {statusBar.engineHealthy ? <StatusPulse tone="ok" /> : <StatusPulse tone="warn" />}
          </div>
        </div>
      </aside>

      <aside className="tcp-context-rail hidden lg:flex xl:hidden">{contextRail(false)}</aside>

      <main
        className={`tcp-main-shell ${
          FULL_HEIGHT_ROUTES.has(currentRoute) ? "tcp-main-shell-fill" : ""
        }`.trim()}
      >
        <section className="tcp-mobile-topbar lg:hidden">
          <button
            type="button"
            className="tcp-btn h-10 px-3"
            disabled={navigationLocked}
            onClick={() => setMobileNavOpen(true)}
          >
            <Icon name="menu" />
            Menu
          </button>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">{routeMeta.title}</div>
            <div className="tcp-subtle truncate text-xs">
              {currentNav?.[1] || routeMeta.subtitle}
            </div>
          </div>
          {incidentMonitorState ? (
            <button
              type="button"
              className={`tcp-incident-monitor-pill ${incidentMonitorState.toneClass}`}
              aria-label={incidentMonitorState.label}
              disabled={navigationLocked}
              title={
                statusBar.incidentMonitor?.lastError
                  ? `${incidentMonitorState.label}: ${statusBar.incidentMonitor.lastError}`
                  : incidentMonitorState.label
              }
              onClick={() => onNavigate("incident-monitor")}
            >
              <Icon name="shield-alert" />
              <span className="tcp-incident-monitor-dot" aria-hidden="true"></span>
            </button>
          ) : null}
          {pendingApprovalCount > 0 ? (
            <button
              type="button"
              className="tcp-approval-pill"
              disabled={navigationLocked}
              title={approvalLabel}
              onClick={() => onNavigate("approvals")}
            >
              <Icon name="shield-alert" />
              <span>{pendingApprovalCount}</span>
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
            <p className="tcp-subtle mt-1 line-clamp-2">{routeMeta.subtitle}</p>
          </div>
          <div className="tcp-topbar-status">
            {incidentMonitorState ? (
              <button
                type="button"
                className={`tcp-incident-monitor-pill ${incidentMonitorState.toneClass}`}
                disabled={navigationLocked}
                title={
                  statusBar.incidentMonitor?.lastError
                    ? `${incidentMonitorState.label}: ${statusBar.incidentMonitor.lastError}`
                    : incidentMonitorState.label
                }
                onClick={() => onNavigate("incident-monitor")}
              >
                <Icon name="shield-alert" />
                <span className="tcp-incident-monitor-dot" aria-hidden="true"></span>
                <span>{incidentMonitorState.shortLabel}</span>
              </button>
            ) : null}
            {pendingApprovalCount > 0 ? (
              <button
                type="button"
                className="tcp-approval-pill"
                disabled={navigationLocked}
                title={approvalLabel}
                onClick={() => onNavigate("approvals")}
              >
                <Icon name="shield-alert" />
                <span>{pendingApprovalCount}</span>
                <span className="hidden sm:inline">
                  Approval{pendingApprovalCount === 1 ? "" : "s"}
                </span>
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

        {providerGate ? <div className="tcp-main-content pb-0">{providerGate}</div> : null}

        <AnimatePresence initial={false} mode="popLayout">
          <motion.section
            key={routeKey}
            className={`tcp-main-content ${
              FULL_HEIGHT_ROUTES.has(currentRoute) ? "tcp-main-content-fill" : ""
            }`.trim()}
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
                <IconButton aria-label="Close navigation" onClick={() => setMobileNavOpen(false)}>
                  <Icon name="x" />
                </IconButton>
              </div>
              {contextRail(true)}
            </motion.aside>
          </motion.div>
        ) : null}
      </AnimatePresence>

      <AnimatePresence>
        {navigationLock ? (
          <motion.div
            className="tcp-confirm-overlay"
            style={{ zIndex: 180 }}
            initial={reducedMotion ? false : { opacity: 0 }}
            animate={reducedMotion ? undefined : { opacity: 1 }}
            exit={reducedMotion ? undefined : { opacity: 0 }}
          >
            <div className="tcp-confirm-backdrop" aria-hidden="true" />
            <motion.div
              className="tcp-confirm-dialog w-[min(36rem,calc(100vw-2rem))]"
              role="alertdialog"
              aria-live="assertive"
              initial={reducedMotion ? false : { opacity: 0, y: 10, scale: 0.985 }}
              animate={reducedMotion ? undefined : { opacity: 1, y: 0, scale: 1 }}
              exit={reducedMotion ? undefined : { opacity: 0, y: 6, scale: 0.985 }}
              transition={
                reducedMotion
                  ? undefined
                  : { duration: MOTION_TOKENS.duration.normal, ease: MOTION_TOKENS.easing.standard }
              }
            >
              <div className="flex items-center gap-3">
                <TandemLogoAnimation className="h-12 w-12 shrink-0" mode="compact" />
                <div className="min-w-0">
                  <h3 className="tcp-confirm-title">{navigationLock.title}</h3>
                  <p className="tcp-confirm-message">{navigationLock.message}</p>
                </div>
              </div>
            </motion.div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
