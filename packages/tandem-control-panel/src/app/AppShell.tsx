import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";

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
  identity: { botName: string; botAvatarUrl: string };
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
  };
  routeKey: string;
  children: any;
  providerGate?: any;
}) {
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const avatarUrl = String(identity.botAvatarUrl || "").trim();
  const [avatarErrored, setAvatarErrored] = useState(false);

  useEffect(() => {
    if (!mobileNavOpen) return;
    setMobileNavOpen(false);
  }, [currentRoute]);

  useEffect(() => {
    setAvatarErrored(false);
  }, [avatarUrl]);

  const renderNavItems = (onDone?: () => void) =>
    navRoutes.map(([id, label, icon]) => {
      const active = currentRoute === id;
      const locked = providerLocked && id !== "settings";
      return (
        <button
          key={id}
          type="button"
          data-route={id}
          disabled={locked}
          onClick={() => {
            onNavigate(id);
            onDone?.();
          }}
          className={`nav-item ${active ? "active" : ""} ${locked ? "locked" : ""} relative`}
        >
          {active ? (
            <motion.span
              layoutId="nav-active-indicator"
              className="absolute inset-0 rounded-lg border border-amber-400/40 bg-amber-400/10"
              transition={{ type: "spring", stiffness: 500, damping: 38 }}
            />
          ) : null}
          <span className="relative z-10 inline-flex items-center gap-2">
            <i data-lucide={icon}></i>
            <span>{label}</span>
          </span>
        </button>
      );
    });

  const renderSidebarBody = (mobile = false) => (
    <>
      <div className="tcp-brand-tile mb-4 flex items-center gap-3 rounded-xl p-3">
        <div className="tcp-brand-avatar grid h-10 w-10 place-items-center overflow-hidden rounded-xl">
          {avatarUrl && !avatarErrored ? (
            <img
              key="brand-avatar-image"
              src={avatarUrl}
              alt={identity.botName}
              className="block h-full w-full object-cover"
              onError={() => setAvatarErrored(true)}
            />
          ) : (
            <span key="brand-avatar-fallback" className="text-xs font-semibold uppercase">
              {String(identity.botName || "T")
                .trim()
                .slice(0, 1) || "T"}
            </span>
          )}
        </div>
        <div>
          <div className="text-base font-semibold">{identity.botName}</div>
          <div className="tcp-subtle text-xs uppercase tracking-wider">Control Center</div>
        </div>
      </div>
      <nav id="nav" className="grid gap-1">
        {renderNavItems(mobile ? () => setMobileNavOpen(false) : undefined)}
      </nav>
      <div className="mt-3 grid gap-2">
        <button
          type="button"
          className="tcp-btn w-full justify-start"
          onClick={() => {
            onPaletteOpen();
            if (mobile) setMobileNavOpen(false);
          }}
        >
          <span className="inline-flex items-center gap-1.5">
            <i data-lucide="search"></i>
            <span>Command Palette</span>
          </span>
          <kbd className="text-[10px] text-slate-400">Ctrl/Cmd+K</kbd>
        </button>
        <button
          type="button"
          className="tcp-btn w-full justify-start"
          onClick={() => {
            onThemeCycle();
            if (mobile) setMobileNavOpen(false);
          }}
        >
          <i data-lucide="sparkles"></i>
          Cycle Theme
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
    </>
  );

  return (
    <div className="grid min-h-screen grid-cols-1 lg:grid-cols-[270px_1fr]">
      <aside className="tcp-sidebar hidden p-4 lg:block">{renderSidebarBody(false)}</aside>
      <main className="flex h-screen min-w-0 flex-col p-3 md:p-5">
        <section className="mb-3 flex items-center justify-between gap-3 rounded-xl border border-slate-700/40 bg-slate-950/30 p-3 lg:hidden">
          <div className="min-w-0">
            <div className="truncate text-sm font-semibold">{identity.botName}</div>
            <div className="tcp-subtle text-[11px] uppercase tracking-wider">Control Center</div>
          </div>
          <button type="button" className="tcp-btn h-9 px-3" onClick={() => setMobileNavOpen(true)}>
            <i data-lucide="menu"></i>
            Menu
          </button>
        </section>
        <section className="mb-3 grid shrink-0 gap-3 rounded-xl border border-slate-700/40 bg-slate-950/30 p-3 md:grid-cols-3">
          <div className="flex items-center justify-between rounded-lg border border-slate-700/50 bg-black/20 p-2">
            <span className="tcp-subtle text-xs">Engine</span>
            <span className={statusBar.engineHealthy ? "tcp-badge-ok" : "tcp-badge-warn"}>
              {statusBar.engineHealthy ? "healthy" : "unknown"}
            </span>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-slate-700/50 bg-black/20 p-2">
            <span className="tcp-subtle text-xs">Provider</span>
            <span className={statusBar.providerBadge}>{statusBar.providerText}</span>
          </div>
          <div className="flex items-center justify-between rounded-lg border border-slate-700/50 bg-black/20 p-2">
            <span className="tcp-subtle text-xs">Active Runs</span>
            <span className="tcp-badge-info">{String(statusBar.activeRuns)}</span>
          </div>
        </section>

        <AnimatePresence mode="wait">
          <motion.section
            key={routeKey}
            className="flex min-h-0 flex-1 flex-col gap-4 tcp-view-surface"
          >
            {children}
          </motion.section>
        </AnimatePresence>
      </main>

      <AnimatePresence>
        {mobileNavOpen ? (
          <motion.div
            className="fixed inset-0 z-50 lg:hidden"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
          >
            <button
              type="button"
              className="absolute inset-0 bg-black/45"
              aria-label="Close menu"
              onClick={() => setMobileNavOpen(false)}
            />
            <motion.aside
              className="tcp-sidebar absolute inset-y-0 left-0 w-[min(84vw,320px)] overflow-auto p-4"
              initial={{ x: "-100%" }}
              animate={{ x: 0 }}
              exit={{ x: "-100%" }}
              transition={{ type: "spring", stiffness: 320, damping: 34 }}
            >
              <div className="mb-3 flex items-center justify-between">
                <div className="text-sm font-semibold">Menu</div>
                <button
                  type="button"
                  className="tcp-btn h-8 px-2"
                  onClick={() => setMobileNavOpen(false)}
                >
                  <i data-lucide="x"></i>
                </button>
              </div>
              {renderSidebarBody(true)}
            </motion.aside>
          </motion.div>
        ) : null}
      </AnimatePresence>

      <AnimatePresence>{providerGate || null}</AnimatePresence>
    </div>
  );
}
