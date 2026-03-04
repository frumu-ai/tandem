import { renderAgents } from "../views/agents.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function AgentsPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="agents" renderer={renderAgents} />;
}
