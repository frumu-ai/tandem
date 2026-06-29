import { AnimatedPage } from "../ui/index.tsx";
import { StatefulRunsPage } from "../features/runs/StatefulRunsPage";
import type { AppPageProps } from "./pageTypes";

export function RunsPage({ api, client, navigate }: AppPageProps) {
  return (
    <AnimatedPage className="grid h-full min-h-0 gap-4">
      <StatefulRunsPage api={api} client={client} navigate={navigate} />
    </AnimatedPage>
  );
}
