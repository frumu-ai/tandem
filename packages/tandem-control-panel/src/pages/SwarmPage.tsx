import { renderSwarm } from "../views/swarm.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function SwarmPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="swarm" renderer={renderSwarm} />;
}
