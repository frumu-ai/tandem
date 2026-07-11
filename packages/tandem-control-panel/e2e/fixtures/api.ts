import { expect, test as base, type Page, type Route } from "@playwright/test";

type HeldRequest = {
  path: string | RegExp;
  release: () => void;
  waitUntilRequested: () => Promise<void>;
};

type PendingHold = {
  matches: (path: string, method: string) => boolean;
  requested: () => void;
  wait: Promise<void>;
};

export type ApiFixture = {
  holdNext: (path: string | RegExp, method?: string) => HeldRequest;
  mockResponse: (path: string | RegExp, response: unknown, method?: string) => void;
  requests: string[];
};

type ResponseOverride = {
  matches: (path: string, method: string) => boolean;
  response: unknown;
};

const jsonHeaders = { "access-control-allow-origin": "*", "content-type": "application/json" };

function responseFor(path: string, method: string): unknown {
  if (path === "/api/auth/me") return { ok: true, user: { id: "e2e-user", name: "E2E User" } };
  if (path === "/api/capabilities") {
    return {
      aca_integration: false,
      coding_workflows: true,
      missions: true,
      agent_teams: true,
      coder: true,
      engine_healthy: true,
      control_panel_mode: "standalone",
      control_panel_config_ready: true,
      workspace_files_available: true,
      workspace_files_api_available: true,
    };
  }
  if (path === "/api/system/health") {
    return { engine: { ready: true, healthy: true }, engineUrl: "fixture://tandem" };
  }
  if (path === "/api/engine/config/providers") {
    return {
      default_provider: "openai",
      default_model: "gpt-5-mini",
      default: "openai",
      providers: { openai: { default_model: "gpt-5-mini", models: ["gpt-5-mini"] } },
    };
  }
  if (path === "/api/engine/provider/auth") {
    return {
      providers: {
        openai: { authenticated: true, connected: true, has_key: true, auth_kind: "api_key" },
      },
    };
  }
  if (path === "/api/engine/provider") {
    return {
      all: [{ id: "openai", name: "OpenAI", models: ["gpt-5-mini"] }],
      connected: ["openai"],
    };
  }
  if (path === "/api/engine/config/identity") {
    return {
      identity: {
        bot: {
          canonical_name: "Tandem",
          avatar_url: "",
          aliases: { control_panel: "Tandem Control Panel" },
        },
      },
    };
  }
  if (path === "/api/install/profile") {
    return {
      aca_integration: false,
      control_panel_mode: "standalone",
      control_panel_config_ready: true,
    };
  }
  if (path === "/api/control-panel/config") {
    return method === "GET"
      ? { config: { mode: "standalone", engine_url: "http://127.0.0.1:39731" } }
      : { ok: true };
  }
  if (path === "/api/system/search-settings") {
    return { available: true, settings: { provider: "none" } };
  }
  if (path === "/api/system/scheduler-settings") {
    return { available: true, settings: { mode: "local", max_concurrent_runs: 1 } };
  }
  if (path === "/api/engine/browser/status") return { installed: false, ready: false };
  if (path === "/api/engine/channels/config" || path === "/api/engine/channels/status") return {};
  if (path === "/api/engine/mcp") return { servers: [] };
  if (path === "/api/engine/mcp/tools") return [];
  if (path === "/api/engine/mcp/catalog") return { servers: [] };
  if (path === "/api/swarm/status" || path === "/api/orchestrator/status") {
    return { status: "idle", runs: [], tasks: [] };
  }
  if (path === "/api/orchestrator/runs") return { runs: [] };
  if (path === "/api/engine/approvals/pending" || path === "/api/aca/approvals/pending") {
    return { approvals: [], count: 0 };
  }
  if (path.includes("/incident-monitor/status")) {
    return {
      status: {
        config: { enabled: false },
        runtime: { pending_incidents: 0, monitoring_active: false, paused: false },
        readiness: { ingest_ready: true, publish_ready: true },
      },
    };
  }
  if (path.includes("/incident-monitor/drafts")) return { drafts: [], items: [] };
  if (path.includes("/incident-monitor/incidents")) return { incidents: [], items: [] };
  if (path.includes("/incident-monitor/posts")) return { posts: [], items: [] };
  if (path.includes("/incident-monitor/intake/keys")) return { keys: [] };
  if (path === "/api/engine/workflows" || path === "/api/engine/automations/v2") return [];
  if (path === "/api/engine/orchestrations") return { orchestrations: [], count: 0 };
  if (path === "/api/engine/goals") return { goals: [], count: 0 };
  if (path.includes("/workflows/runs") || path.includes("/automations/v2/runs")) {
    return { runs: [], items: [] };
  }
  if (path === "/api/engine/context/packs" || path === "/api/engine/packs") {
    return { packs: [], items: [] };
  }
  if (path === "/api/engine/context/runs") return { runs: [], items: [] };
  if (path === "/api/engine/session") return { sessions: [], count: 0 };
  if (path === "/api/files" || path === "/api/workspace/files") return { files: [], entries: [] };
  if (path === "/api/knowledgebase/config") return { configured: false };
  if (path === "/api/knowledgebase/collections") return { collections: [] };
  if (path === "/api/knowledgebase/documents") return { documents: [], items: [] };
  if (path === "/api/knowledgebase/prompts") return { prompts: [], items: [] };
  if (path.includes("/stateful-runtime/runs")) return { runs: [], items: [], queues: [] };
  if (path.includes("/stateful-runtime/reliability")) return { metrics: {}, incidents: [] };
  if (path.startsWith("/api/engine/enterprise/")) return { items: [], data: [] };
  if (path.startsWith("/api/aca/")) return { projects: [], runs: [], approvals: [], items: [] };
  return method === "GET" ? {} : { ok: true };
}

async function fulfillApi(
  route: Route,
  pending: PendingHold[],
  overrides: ResponseOverride[],
  requests: string[]
) {
  const request = route.request();
  const url = new URL(request.url());
  const path = url.pathname;
  requests.push(`${request.method()} ${path}${url.search}`);
  const heldIndex = pending.findIndex((entry) => entry.matches(path, request.method()));
  if (heldIndex >= 0) {
    const [held] = pending.splice(heldIndex, 1);
    held.requested();
    await held.wait;
  }
  const override = [...overrides].reverse().find((entry) => entry.matches(path, request.method()));
  await route.fulfill({
    status: 200,
    headers: jsonHeaders,
    body: JSON.stringify(override ? override.response : responseFor(path, request.method())),
  });
}

export const test = base.extend<{ apiFixture: ApiFixture }>({
  apiFixture: [
    async ({ page }, use) => {
      const pending: PendingHold[] = [];
      const overrides: ResponseOverride[] = [];
      const requests: string[] = [];
      await page.addInitScript(() => {
        localStorage.clear();
        sessionStorage.clear();
        localStorage.setItem("tandem.navigationVisibility.v1", JSON.stringify({}));
      });
      await page.route("**/api/**", (route) => fulfillApi(route, pending, overrides, requests));
      await use({
        requests,
        mockResponse(path, response, method) {
          overrides.push({
            matches: (candidate, candidateMethod) =>
              (!method || candidateMethod === method) &&
              (typeof path === "string" ? candidate === path : path.test(candidate)),
            response,
          });
        },
        holdNext(path, method) {
          let releaseRequest!: () => void;
          let markRequested!: () => void;
          const wait = new Promise<void>((resolve) => (releaseRequest = resolve));
          const requested = new Promise<void>((resolve) => (markRequested = resolve));
          pending.push({
            matches: (candidate, candidateMethod) =>
              (!method || candidateMethod === method) &&
              (typeof path === "string" ? candidate === path : path.test(candidate)),
            requested: markRequested,
            wait,
          });
          return {
            path,
            release: releaseRequest,
            waitUntilRequested: () => requested,
          };
        },
      });
    },
    { auto: true },
  ],
});

export { expect };

export async function waitForRoute(page: Page, routeId: string) {
  const marker = page.locator(`[data-testid="route-outlet"][data-route-id="${routeId}"]`);
  await expect(marker).toHaveCount(1);
  await expect(page.getByTestId("route-outlet")).toHaveCount(1);
  await expect(page.getByText("Page failed to load", { exact: true })).toHaveCount(0);
  await page.waitForFunction(
    () => new Promise((resolve) => requestAnimationFrame(() => resolve(true)))
  );
}

export async function blankIconDescriptions(page: Page) {
  return page.locator("svg").evaluateAll((icons) =>
    icons.flatMap((icon, index) => {
      const isLucide = icon.classList.contains("lucide");
      const isUnknownFallback =
        icon.getAttribute("aria-hidden") === "true" &&
        icon.hasAttribute("width") &&
        icon.hasAttribute("height") &&
        !icon.hasAttribute("viewBox") &&
        icon.childElementCount === 0;
      if ((!isLucide && !isUnknownFallback) || icon.childElementCount > 0) return [];
      return [
        `${index}: class=${JSON.stringify(icon.getAttribute("class"))} outerHTML=${icon.outerHTML}`,
      ];
    })
  );
}
