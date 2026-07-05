import { Component, Suspense, lazy } from "react";
import { ensureRouteId } from "./routes";
import { ApprovalsInboxPage } from "../pages/ApprovalsInboxPage";

const lazyNamed = <K extends string, M extends Record<K, any>>(loader: () => Promise<M>, name: K) =>
  lazy(() => loader().then((m) => ({ default: m[name] })));

const DashboardPage = lazyNamed(() => import("../pages/DashboardPage"), "DashboardPage");
const ChatPage = lazyNamed(() => import("../pages/ChatPage"), "ChatPage");
const IntentPlannerPage = lazyNamed(
  () => import("../pages/IntentPlannerPage"),
  "IntentPlannerPage"
);
const WorkflowsPage = lazyNamed(() => import("../pages/WorkflowsPage"), "WorkflowsPage");
const MarketplacePage = lazyNamed(() => import("../pages/MarketplacePage"), "MarketplacePage");
const WorkflowStudioPage = lazyNamed(
  () => import("../pages/WorkflowStudioPage"),
  "WorkflowStudioPage"
);
const AutomationsPage = lazyNamed(() => import("../pages/AutomationsPage"), "AutomationsPage");
const WebhooksPage = lazyNamed(() => import("../pages/WebhooksPage"), "WebhooksPage");
const ExperimentsPage = lazyNamed(() => import("../pages/ExperimentsPage"), "ExperimentsPage");
const EnterpriseAdminPage = lazyNamed(
  () => import("../pages/EnterpriseAdminPage"),
  "EnterpriseAdminPage"
);
const CodingWorkflowsPage = lazyNamed(
  () => import("../pages/CodingWorkflowsPage"),
  "CodingWorkflowsPage"
);
const ChannelsPage = lazyNamed(() => import("../pages/ChannelsPage"), "ChannelsPage");
const PacksPage = lazyNamed(() => import("../pages/PacksPage"), "PacksPage");
const OrchestratorPage = lazyNamed(() => import("../pages/OrchestratorPage"), "OrchestratorPage");
const FilesPage = lazyNamed(() => import("../pages/FilesPage"), "FilesPage");
const MemoryPage = lazyNamed(() => import("../pages/MemoryPage"), "MemoryPage");
const RunsPage = lazyNamed(() => import("../pages/RunsPage"), "RunsPage");
const ControlLoopPage = lazyNamed(() => import("../pages/ControlLoopPage"), "ControlLoopPage");
const IncidentMonitorPage = lazyNamed(
  () => import("../pages/IncidentMonitorPage"),
  "IncidentMonitorPage"
);
const TeamsPage = lazyNamed(() => import("../pages/TeamsPage"), "TeamsPage");
const SettingsPage = lazyNamed(() => import("../pages/SettingsPage"), "SettingsPage");

function RouteFallback() {
  return (
    <div className="flex min-h-[40vh] items-center justify-center">
      <div className="tcp-subtle text-sm">Loading…</div>
    </div>
  );
}

class LazyRouteErrorBoundary extends Component<{ children: any }, { error: Error | null }> {
  state = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (!this.state.error) return this.props.children;
    const message = String(this.state.error?.message || this.state.error || "");
    const looksLikeStaleChunk =
      message.includes("dynamically imported module") ||
      message.includes("Loading chunk") ||
      message.includes("Importing a module script failed");
    return (
      <div className="grid min-h-[40vh] place-items-center p-6">
        <div className="max-w-lg rounded-xl border border-amber-500/30 bg-amber-500/10 p-4">
          <div className="mb-2 font-semibold text-amber-100">
            {looksLikeStaleChunk ? "Panel update detected" : "Page failed to load"}
          </div>
          <p className="tcp-subtle text-sm">
            {looksLikeStaleChunk
              ? "This page chunk was rebuilt while your browser still had the old file name cached. Reload the control panel to fetch the current assets."
              : message || "The route could not be loaded."}
          </p>
          <button className="tcp-btn mt-3" onClick={() => window.location.reload()}>
            Reload control panel
          </button>
        </div>
      </div>
    );
  }
}

function renderRoute(routeId: ReturnType<typeof ensureRouteId>, pageProps: any) {
  switch (routeId) {
    case "chat":
      return <ChatPage {...pageProps} />;
    case "planner":
      return <IntentPlannerPage {...pageProps} />;
    case "workflows":
      return <WorkflowsPage {...pageProps} />;
    case "marketplace":
      return <MarketplacePage {...pageProps} />;
    case "studio":
      return <WorkflowStudioPage {...pageProps} />;
    case "automations":
    case "packs":
    case "teams":
      return <AutomationsPage {...pageProps} />;
    case "webhooks":
      return <WebhooksPage {...pageProps} />;
    case "experiments":
      return <ExperimentsPage {...pageProps} />;
    case "enterprise-admin":
      return <EnterpriseAdminPage {...pageProps} />;
    case "coding":
      return <CodingWorkflowsPage {...pageProps} />;
    case "agents":
      return <TeamsPage {...pageProps} />;
    case "channels":
      return <ChannelsPage {...pageProps} />;
    case "mcp":
      return <SettingsPage {...pageProps} />;
    case "packs-detail":
      return <PacksPage {...pageProps} />;
    case "orchestrator":
      return <OrchestratorPage {...pageProps} />;
    case "incident-monitor":
      return <IncidentMonitorPage {...pageProps} />;
    case "files":
      return <FilesPage {...pageProps} />;
    case "memory":
      return <MemoryPage {...pageProps} />;
    case "teams-detail":
      return <TeamsPage {...pageProps} />;
    case "runs":
      return <RunsPage {...pageProps} />;
    case "control-loop":
      return <ControlLoopPage {...pageProps} />;
    case "approvals":
      return <ApprovalsInboxPage {...pageProps} />;
    case "settings":
      return <SettingsPage {...pageProps} />;
    case "dashboard":
    default:
      return <DashboardPage {...pageProps} />;
  }
}

export function HashRouteOutlet({ routeId, pageProps }: { routeId: string; pageProps: any }) {
  const safeRoute = ensureRouteId(routeId);
  return (
    <LazyRouteErrorBoundary key={safeRoute}>
      <Suspense fallback={<RouteFallback />}>{renderRoute(safeRoute, pageProps)}</Suspense>
    </LazyRouteErrorBoundary>
  );
}
