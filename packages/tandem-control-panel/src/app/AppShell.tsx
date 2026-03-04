import { AnimatePresence, motion } from "motion/react";

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
  return (
    <div className="grid min-h-screen grid-cols-1 lg:grid-cols-[270px_1fr]">
      <aside className="tcp-sidebar p-4">
        <div className="tcp-brand-tile mb-4 flex items-center gap-3 rounded-xl p-3">
          <div className="tcp-brand-avatar grid h-10 w-10 place-items-center overflow-hidden rounded-xl">
            {identity.botAvatarUrl ? (
              <img
                src={identity.botAvatarUrl}
                alt={identity.botName}
                className="h-full w-full object-cover"
              />
            ) : (
              <i data-lucide="cpu"></i>
            )}
          </div>
          <div>
            <div className="text-base font-semibold">{identity.botName}</div>
            <div className="tcp-subtle text-xs uppercase tracking-wider">Control Center</div>
          </div>
        </div>
        <nav id="nav" className="grid gap-1">
          {navRoutes.map(([id, label, icon]) => {
            const active = currentRoute === id;
            const locked = providerLocked && id !== "settings";
            return (
              <button
                key={id}
                type="button"
                data-route={id}
                disabled={locked}
                onClick={() => onNavigate(id)}
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
          })}
        </nav>
        <div className="mt-3 grid gap-2">
          <button type="button" className="tcp-btn" onClick={onPaletteOpen}>
            <span>Command Palette</span>
            <kbd className="text-[10px] text-slate-400">Ctrl/Cmd+K</kbd>
          </button>
          <button type="button" className="tcp-btn" onClick={onThemeCycle}>
            Cycle Theme
          </button>
          <button type="button" className="tcp-btn" onClick={onLogout}>
            Logout
          </button>
        </div>
      </aside>
      <main className="min-w-0 p-3 md:p-5">
        <section className="mb-3 grid gap-3 rounded-xl border border-slate-700/40 bg-slate-950/30 p-3 md:grid-cols-3">
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
            className="grid h-[calc(100vh-10.5rem)] gap-4 tcp-view-surface"
          >
            {children}
          </motion.section>
        </AnimatePresence>
      </main>

      <AnimatePresence>{providerGate || null}</AnimatePresence>
    </div>
  );
}
