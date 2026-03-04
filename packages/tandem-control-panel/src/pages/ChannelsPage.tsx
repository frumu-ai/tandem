import { renderChannels } from "../views/channels.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function ChannelsPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="channels" renderer={renderChannels} />;
}
