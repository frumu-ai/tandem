import { renderMcp } from "../views/mcp.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function McpPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="mcp" renderer={renderMcp} />;
}
