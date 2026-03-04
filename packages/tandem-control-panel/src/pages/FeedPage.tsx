import { renderFeed } from "../views/feed.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function FeedPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="feed" renderer={renderFeed} />;
}
