import Router from "preact-router";
import { DashboardPage } from "../pages/DashboardPage";
import { ChatPage } from "../pages/ChatPage";
import { AutomationsPage } from "../pages/AutomationsPage";
import { ChannelsPage } from "../pages/ChannelsPage";
import { McpPage } from "../pages/McpPage";
import { PacksPage } from "../pages/PacksPage";
import { SwarmPage } from "../pages/SwarmPage";
import { FilesPage } from "../pages/FilesPage";
import { MemoryPage } from "../pages/MemoryPage";
import { TeamsPage } from "../pages/TeamsPage";
import { FeedPage } from "../pages/FeedPage";
import { SettingsPage } from "../pages/SettingsPage";

export function HashRouteOutlet({ routeId, pageProps }: { routeId: string; pageProps: any }) {
  return (
    <Router url={`/${routeId}`}>
      <DashboardPage path="/dashboard" {...pageProps} />
      <ChatPage path="/chat" {...pageProps} />
      <AutomationsPage path="/automations" {...pageProps} />
      {/* Legacy routes→automations for backwards compat */}
      <AutomationsPage path="/agents" {...pageProps} />
      <AutomationsPage path="/packs" {...pageProps} />
      <AutomationsPage path="/teams" {...pageProps} />
      <ChannelsPage path="/channels" {...pageProps} />
      <McpPage path="/mcp" {...pageProps} />
      <PacksPage path="/packs-detail" {...pageProps} />
      <SwarmPage path="/swarm" {...pageProps} />
      <FilesPage path="/files" {...pageProps} />
      <MemoryPage path="/memory" {...pageProps} />
      <TeamsPage path="/teams-detail" {...pageProps} />
      <FeedPage path="/feed" {...pageProps} />
      <SettingsPage path="/settings" {...pageProps} />
      <DashboardPage default {...pageProps} />
    </Router>
  );
}
