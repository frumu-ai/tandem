import { renderPacks } from "../views/packs.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function PacksPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="packs" renderer={renderPacks} />;
}
