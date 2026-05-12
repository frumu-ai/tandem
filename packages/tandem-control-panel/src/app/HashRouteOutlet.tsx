import { Suspense, lazy } from "react";
import { ensureRouteId } from "./routes";

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
const ExperimentsPage = lazyNamed(() => import("../pages/ExperimentsPage"), "ExperimentsPage");
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
const ApprovalsInboxPage = lazyNamed(
  () => import("../pages/ApprovalsInboxPage"),
  "ApprovalsInboxPage"
);
const BugMonitorPage = lazyNamed(() => import("../pages/BugMonitorPage"), "BugMonitorPage");
const TeamsPage = lazyNamed(() => import("../pages/TeamsPage"), "TeamsPage");
const SettingsPage = lazyNamed(() => import("../pages/SettingsPage"), "SettingsPage");

function RouteFallback() {
  return (
    <div className="flex min-h-[40vh] items-center justify-center">
      <div className="tcp-subtle text-sm">Loading…</div>
    </div>
  );
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
    case "experiments":
      return <ExperimentsPage {...pageProps} />;
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
    case "bug-monitor":
      return <BugMonitorPage {...pageProps} />;
    case "files":
      return <FilesPage {...pageProps} />;
    case "memory":
      return <MemoryPage {...pageProps} />;
    case "teams-detail":
      return <TeamsPage {...pageProps} />;
    case "runs":
      return <RunsPage {...pageProps} />;
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
    <Suspense key={safeRoute} fallback={<RouteFallback />}>
      {renderRoute(safeRoute, pageProps)}
    </Suspense>
  );
}
