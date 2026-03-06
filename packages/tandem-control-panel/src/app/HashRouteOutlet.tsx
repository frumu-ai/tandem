import { DashboardPage } from "../pages/DashboardPage";
import { ChatPage } from "../pages/ChatPage";
import { AutomationsPage } from "../pages/AutomationsPage";
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
    case "automations":
    case "agents":
    case "packs":
    case "teams":
      return <AutomationsPage {...pageProps} />;
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
