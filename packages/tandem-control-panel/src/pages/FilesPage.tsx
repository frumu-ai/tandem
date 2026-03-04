import { renderFiles } from "../views/files.js";
import { LegacyPage } from "./LegacyPage";
import type { RoutablePageProps } from "./pageTypes";

export function FilesPage(props: RoutablePageProps) {
  return <LegacyPage {...props} routeId="files" renderer={renderFiles} />;
}
