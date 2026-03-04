import { renderMemory } from "../views/memory.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function MemoryPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="memory" renderer={renderMemory} />;
}
