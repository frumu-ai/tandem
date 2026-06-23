import { describe, expect, it } from "vitest";
import { TandemClient } from "../src/client.js";
import type {
  BugMonitorConfigResponse,
  BugMonitorIntakeKeyCreateInput,
  BugMonitorIntakeKeyCreateResponse,
  BugMonitorIntakeKeyDisableResponse,
  BugMonitorIntakeKeyListResponse,
  BugMonitorLogSourceReplayResponse,
  BugMonitorLogSourceResetResponse,
  BugMonitorPostRecord,
  BugMonitorRoutePreviewResponse,
  BugMonitorStatusResponse,
} from "../src/public/index.js";

describe("Bug Monitor external project public types", () => {
  it("accept monitored project config and structured log watcher status", () => {
    const config: BugMonitorConfigResponse = {
      bug_monitor: {
        enabled: true,
        repo: "frumu-ai/tandem",
        destinations: [
          {
            destination_id: "legacy-github",
            name: "GitHub (legacy Bug Monitor)",
            kind: "github_issue",
            enabled: true,
            repo: "frumu-ai/tandem",
            mcp_server: "github",
            route_tags: ["legacy_github"],
          },
        ],
        routes: [
          {
            route_id: "default",
            name: "Default route",
            enabled: true,
            destination_ids: ["legacy-github"],
            approval_policy: "inherit",
            match_sources: ["manual"],
            match_source_kinds: ["ci"],
            match_route_tags: ["payments"],
          },
        ],
        default_destination_ids: ["legacy-github"],
        safety_defaults: {
          require_approval_for_high_risk: true,
          redact_secrets: true,
          block_unready_destinations: false,
        },
        monitored_projects: [
          {
            project_id: "aca",
            name: "ACA",
            enabled: true,
            repo: "frumu-ai/aca",
            workspace_root: "/home/evan/aca",
            source_kind: "external_app",
            mcp_server: "github",
            allowed_destination_ids: ["legacy-github"],
            default_destination_ids: ["legacy-github"],
            default_route_tags: ["aca"],
            tenant_id: "tenant-a",
            approval_policy: "high_risk",
            log_sources: [
              {
                source_id: "coder-worker",
                path: "logs/coder-worker.jsonl",
                source_kind: "ci",
                format: "json",
                minimum_level: "error",
                start_position: "end",
                watch_interval_seconds: 5,
                default_route_tags: ["worker"],
                workspace_id: "workspace-a",
              },
            ],
          },
        ],
      },
    };
    const status: BugMonitorStatusResponse = {
      status: {
        config: config.bug_monitor,
        destinations: config.bug_monitor.destinations,
        destination_readiness: [
          {
            destination_id: "legacy-github",
            kind: "github_issue",
            enabled: true,
            ready: true,
            publish_ready: true,
            requires_approval: false,
          },
        ],
        log_watcher: {
          running: true,
          enabled_projects: 1,
          enabled_sources: 1,
          sources: [
            {
              project_id: "aca",
              source_id: "coder-worker",
              path: "/home/evan/aca/logs/coder-worker.jsonl",
              healthy: true,
              offset: 2048,
              file_size: 4096,
              total_candidates: 1,
              total_submitted: 1,
            },
          ],
        },
      },
    };
    const preview: BugMonitorRoutePreviewResponse = {
      matches: [
        {
          route_id: "default",
          destination_ids: ["legacy-github"],
          approval_required: false,
          reason: "default_destination",
        },
      ],
      destinations: config.bug_monitor.destinations,
      readiness: status.status.destination_readiness,
      default_destination_ids: ["legacy-github"],
      effective_destination_ids: ["legacy-github"],
      approval_required: false,
      blocked: false,
      blocked_reasons: [],
    };
    const post: BugMonitorPostRecord = {
      post_id: "post-1",
      draft_id: "draft-1",
      repo: "frumu-ai/tandem",
      operation: "create_issue",
      status: "posted",
      destination_id: "legacy-github",
      destination_kind: "github_issue",
      route_match_reason: "legacy_github",
      external_id: "42",
      external_url: "https://github.com/frumu-ai/tandem/issues/42",
      external_title: "GitHub issue #42",
      target_ref: "frumu-ai/tandem",
      receipt: { provider: "github", issue_number: 42 },
    };

    expect(config.bug_monitor.monitored_projects?.[0]?.log_sources?.[0]?.source_id).toBe(
      "coder-worker"
    );
    expect(config.bug_monitor.monitored_projects?.[0]?.source_kind).toBe("external_app");
    expect(config.bug_monitor.monitored_projects?.[0]?.log_sources?.[0]?.source_kind).toBe("ci");
    expect(status.status.log_watcher?.sources?.[0]?.healthy).toBe(true);
    expect(preview.effective_destination_ids?.[0]).toBe("legacy-github");
    expect(post.receipt && typeof post.receipt === "object").toBe(true);
  });

  it("accepts scoped intake key management payloads", () => {
    const createInput: BugMonitorIntakeKeyCreateInput = {
      project_id: "aca",
      name: "ACA CI",
      scopes: ["bug_monitor:report"],
    };
    const listResponse: BugMonitorIntakeKeyListResponse = {
      keys: [
        {
          key_id: "intake-key-1",
          project_id: "aca",
          name: "ACA CI",
          key_hash: "[redacted]",
          enabled: true,
          scopes: ["bug_monitor:report"],
          created_at_ms: 1,
          last_used_at_ms: null,
        },
      ],
    };
    const createResponse: BugMonitorIntakeKeyCreateResponse = {
      key: listResponse.keys[0]!,
      raw_key: "tbm_intake_secret",
    };
    const disableResponse: BugMonitorIntakeKeyDisableResponse = {
      key: { ...listResponse.keys[0]!, enabled: false },
    };

    expect(createInput.project_id).toBe("aca");
    expect(createResponse.raw_key).toContain("tbm_intake_");
    expect(disableResponse.key.enabled).toBe(false);
  });

  it("calls scoped intake key endpoints with typed payloads", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string; body?: string }> = [];
    globalThis.fetch = (async (input, init) => {
      calls.push({
        url: String(input),
        method: String(init?.method ?? "GET"),
        body: typeof init?.body === "string" ? init.body : undefined,
      });
      return new Response(
        JSON.stringify({
          keys: [],
          key: {
            key_id: "intake-key-1",
            project_id: "aca",
            name: "ACA CI",
            key_hash: "[redacted]",
            enabled: true,
            scopes: ["bug_monitor:report"],
          },
          raw_key: "tbm_intake_secret",
        }),
        {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }
      );
    }) as typeof fetch;

    try {
      await client.bugMonitor.listIntakeKeys();
      await client.bugMonitor.createIntakeKey({
        project_id: "aca",
        name: "ACA CI",
        scopes: ["bug_monitor:report"],
      });
      await client.bugMonitor.disableIntakeKey("intake/key 1");
      await client.bugMonitor.resetLogSourceOffset("aca/project", "worker/source");
      await client.bugMonitor.replayLatestLogSourceCandidate("aca/project", "worker/source");
      await client.bugMonitor.previewRoute({
        source: "desktop_logs",
        risk_level: "high",
      });
      await client.bugMonitor.listPosts({
        limit: 25,
        destinationId: "legacy-github",
      });

      expect(calls[0]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/intake/keys",
        method: "GET",
      });
      expect(calls[1]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/intake/keys",
        method: "POST",
      });
      expect(calls[1]?.body).toBe(
        JSON.stringify({
          project_id: "aca",
          name: "ACA CI",
          scopes: ["bug_monitor:report"],
        })
      );
      expect(calls[2]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/intake/keys/intake%2Fkey%201/disable",
        method: "POST",
      });
      expect(calls[3]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/log-sources/aca%2Fproject/worker%2Fsource/reset-offset",
        method: "POST",
      });
      expect(calls[4]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/log-sources/aca%2Fproject/worker%2Fsource/replay-latest",
        method: "POST",
      });
      expect(calls[5]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/route-preview",
        method: "POST",
      });
      expect(calls[5]?.body).toBe(
        JSON.stringify({
          source: "desktop_logs",
          risk_level: "high",
        })
      );
      expect(calls[6]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/posts?limit=25&destination_id=legacy-github",
        method: "GET",
      });
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("accepts log-source debug action responses", () => {
    const reset: BugMonitorLogSourceResetResponse = {
      project_id: "aca",
      source_id: "worker",
      state: {
        project_id: "aca",
        source_id: "worker",
        path: "/tmp/worker.log",
        offset: 0,
        total_candidates: 3,
      },
    };
    const replay: BugMonitorLogSourceReplayResponse = {
      project_id: "aca",
      source_id: "worker",
      incident: {
        incident_id: "failure-incident-1",
        fingerprint: "fp",
        event_type: "external_service_crash",
        status: "draft_created",
        repo: "frumu-ai/aca",
        workspace_root: "/home/evan/aca",
        title: "External service crashed",
        occurrence_count: 2,
        created_at_ms: 1,
        updated_at_ms: 2,
      },
      draft: null,
    };

    expect(reset.state.offset).toBe(0);
    expect(replay.incident.occurrence_count).toBe(2);
  });
});
