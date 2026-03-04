import { renderDashboard } from "../views/dashboard.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function DashboardPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="dashboard" renderer={renderDashboard} />;
}
