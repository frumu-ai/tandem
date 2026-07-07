import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { groupNavRoutes, NAV_ROUTES, ROUTES } from "../src/app/store.js";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "..", "src");

test("Slack governance receipts route is reachable from navigation and router source", () => {
  const route = ROUTES.find(([id]) => id === "slack-receipts");
  assert.deepEqual(route, ["slack-receipts", "Slack Receipts", "file-check-2"]);

  assert.ok(
    NAV_ROUTES.some(([id]) => id === "slack-receipts"),
    "Slack receipts must be in primary navigation",
  );

  const governGroup = groupNavRoutes(NAV_ROUTES).find((group) => group.label === "Govern");
  assert.ok(governGroup, "Govern navigation group must exist");
  assert.ok(
    governGroup.items.some(([id]) => id === "slack-receipts"),
    "Slack receipts belongs with other governance surfaces",
  );

  const routeTypes = readFileSync(join(srcDir, "app", "routes.ts"), "utf8");
  assert.match(routeTypes, /\|\s*"slack-receipts"/, "RouteId union must include slack-receipts");

  const outlet = readFileSync(join(srcDir, "app", "HashRouteOutlet.tsx"), "utf8");
  assert.match(
    outlet,
    /import\("\.\.\/pages\/SlackGovernanceReceiptPage"\)/,
    "route outlet must lazy-load the receipt page",
  );
  assert.match(
    outlet,
    /case "slack-receipts":[\s\S]*?<SlackGovernanceReceiptPage \{\.\.\.pageProps\} \/>/,
    "route outlet must render the receipt page for slack-receipts",
  );
});
