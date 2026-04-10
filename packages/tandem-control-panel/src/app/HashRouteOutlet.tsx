import { DashboardPage } from "../pages/DashboardPage";
import { ChatPage } from "../pages/ChatPage";
import { IntentPlannerPage } from "../pages/IntentPlannerPage";
import { WorkflowsPage } from "../pages/WorkflowsPage";
import { MarketplacePage } from "../pages/MarketplacePage";
import { WorkflowStudioPage } from "../pages/WorkflowStudioPage";
import { AutomationsPage } from "../pages/AutomationsPage";
import { CodingWorkflowsPage } from "../pages/CodingWorkflowsPage";
import { ChannelsPage } from "../pages/ChannelsPage";
import { PacksPage } from "../pages/PacksPage";
import { OrchestratorPage } from "../pages/OrchestratorPage";
import { FilesPage } from "../pages/FilesPage";
import { MemoryPage } from "../pages/MemoryPage";
import { TeamsPage } from "../pages/TeamsPage";
import { FeedPage } from "../pages/FeedPage";
import { SettingsPage } from "../pages/SettingsPage";
import { ensureRouteId } from "./routes";

export function HashRouteOutlet({ routeId, pageProps }: { routeId: string; pageProps: any }) {
  const safeRoute = ensureRouteId(routeId);

  switch (safeRoute) {
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
    case "files":
      return <FilesPage {...pageProps} />;
    case "memory":
      return <MemoryPage {...pageProps} />;
    case "teams-detail":
      return <TeamsPage {...pageProps} />;
    case "feed":
      return <FeedPage {...pageProps} />;
    case "settings":
      return <SettingsPage {...pageProps} />;
    case "dashboard":
    default:
      return <DashboardPage {...pageProps} />;
  }
}
