import { renderTeams } from "../views/teams.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function TeamsPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="teams" renderer={renderTeams} />;
}
