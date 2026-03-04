import { renderChat } from "../views/chat.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function ChatPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="chat" renderer={renderChat} />;
}
