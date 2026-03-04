import { renderSettings } from "../views/settings.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function SettingsPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="settings" renderer={renderSettings} />;
}
