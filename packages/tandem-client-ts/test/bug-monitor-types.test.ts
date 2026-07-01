import { describe, expect, it } from "vitest";
import { TandemClient } from "../src/client.js";
import type {
  BugMonitorAuthorityInventoryResponse,
  BugMonitorAssessmentReportResponse,
  BugMonitorAssessmentProbeRunResponse,
  BugMonitorConfigResponse,
  BugMonitorDestinationConfig,
  BugMonitorIntakeKeyCreateInput,
  BugMonitorIntakeKeyCreateResponse,
  BugMonitorIntakeKeyDisableResponse,
  BugMonitorIntakeKeyListResponse,
  BugMonitorLogSourceReplayResponse,
  BugMonitorLogSourceResetResponse,
  BugMonitorPostureChecksResponse,
  BugMonitorPostRecord,
  BugMonitorRouteConfig,
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

  it("fetches the authority inventory through the SDK helper", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string }> = [];
    const response: BugMonitorAuthorityInventoryResponse = {
      schema_version: 1,
      scope: {
        source: "bug_monitor_authority_inventory",
        read_only: true,
      },
      inventory: {
        workflows: [{ workflow_id: "wf-1", enabled: true }],
        automation_specs: [{ automation_id: "auto-1", agents: [{ agent_id: "agent-1" }] }],
        mcp: { servers: [{ server: "github", tool_count: 1 }] },
        destinations: [{ destination_id: "linear-prod", kind: "linear_issue", require_approval: true }],
        routes: [{ route_id: "high-risk", destination_ids: ["linear-prod"] }],
        monitored_sources: [{ project_id: "payments", source_kind: "ci" }],
        scoped_intake_keys: [{ key_id: "key-1", project_id: "payments", key_hash_present: true }],
        approval_rules: [{ rule_id: "destination:linear-prod", requires_approval: true }],
        external_publish_surfaces: {
          configured_destinations: [{ surface_id: "linear-prod" }],
        },
      },
      counts: {
        workflows: 1,
        automation_specs: 1,
        destinations: 1,
      },
      sensitive_values: {
        policy: "redacted_or_summarized",
      },
    };

    globalThis.fetch = (async (input, init) => {
      calls.push({ url: String(input), method: String(init?.method ?? "GET") });
      return new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const inventory = await client.bugMonitor.getAuthorityInventory();
      expect(inventory.inventory.scoped_intake_keys?.[0]?.key_hash_present).toBe(true);
      expect(inventory.inventory.destinations?.[0]?.require_approval).toBe(true);
      expect(calls[0]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/security/authority-inventory",
        method: "GET",
      });
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("fetches security posture checks through the SDK helper", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string }> = [];
    const response: BugMonitorPostureChecksResponse = {
      schema_version: 1,
      scope: {
        source: "bug_monitor_security_posture_checks",
        read_only: true,
        dry_run: true,
      },
      baseline_policy: {
        mode: "dry_run",
      },
      findings: [
        {
          finding_id: "bpf_123",
          fingerprint: "sha256:abc",
          rule_id: "mcp_server_without_tool_allowlist",
          category: "mcp_allowlist_gap",
          severity: "high",
          title: "MCP server missing tool allowlist",
          incident_draft_suggestion: {
            source: "security_posture",
            event_type: "security.posture.finding",
          },
        },
      ],
      counts: {
        findings: 1,
        by_severity: { high: 1 },
        by_category: { mcp_allowlist_gap: 1 },
      },
    };

    globalThis.fetch = (async (input, init) => {
      calls.push({ url: String(input), method: String(init?.method ?? "GET") });
      return new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const posture = await client.bugMonitor.getPostureChecks({
        rules: ["mcp_server_without_tool_allowlist"],
        minSeverity: "medium",
      });
      expect(posture.findings[0]?.category).toBe("mcp_allowlist_gap");
      expect(posture.counts?.by_severity?.high).toBe(1);
      expect(calls[0]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/security/posture-checks?rules=mcp_server_without_tool_allowlist&min_severity=medium",
        method: "GET",
      });
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("runs security assessment probes through the SDK helper", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string; body?: string }> = [];
    const response: BugMonitorAssessmentProbeRunResponse = {
      schema_version: 1,
      scope: {
        source: "bug_monitor_security_assessment_probes",
        read_only: true,
        dry_run: true,
      },
      probe_policy: {
        mode: "dry_run",
        selected_probe_ids: ["webhook_url_policy"],
      },
      results: [
        {
          probe_id: "webhook_url_policy",
          status: "fail",
          expected_behavior: "Webhook destinations must use public HTTPS URLs.",
          observed_behavior: "Webhook URL points to localhost/private network",
          incident_draft_suggestion: {
            source: "security_assessment_probe",
          },
        },
      ],
      counts: {
        results: 1,
        fail: 1,
        by_status: { fail: 1 },
        draft_suggestions: 1,
      },
      evidence_pack: {
        persisted: true,
        context_run_id: "bug-monitor-assessment-probes-1",
      },
    };

    globalThis.fetch = (async (input, init) => {
      calls.push({
        url: String(input),
        method: String(init?.method ?? "GET"),
        body: String(init?.body ?? ""),
      });
      return new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const probes = await client.bugMonitor.runAssessmentProbes({
        probes: ["webhook_url_policy"],
      });
      expect(probes.results[0]?.probe_id).toBe("webhook_url_policy");
      expect(probes.counts?.fail).toBe(1);
      expect(calls[0]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/security/assessment-probes",
        method: "POST",
      });
      expect(JSON.parse(calls[0]?.body ?? "{}")).toEqual({
        probes: ["webhook_url_policy"],
      });
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("generates security assessment reports through the SDK helper", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string; body?: string }> = [];
    const response: BugMonitorAssessmentReportResponse = {
      schema_version: 1,
      scope: {
        source: "bug_monitor_security_gap_assessment_report",
        read_only: true,
      },
      counts: {
        findings: 1,
        protected_audit_events: 1,
      },
      sections: {
        self_monitoring_boundary: {
          source_kinds: ["tandem_runtime", "tandem_monitor"],
          external_export_required_for_high_assurance: true,
        },
        external_audit_export: {
          existing_ndjson_endpoint: "/audit/export",
          records: [{ event_type: "bug_monitor.publish.failed" }],
        },
      },
      markdown_summary: "# Incident Monitor Security Gap Assessment",
      evidence_pack: {
        persisted: true,
        context_run_id: "bug-monitor-assessment-report-1",
      },
    };

    globalThis.fetch = (async (input, init) => {
      calls.push({
        url: String(input),
        method: String(init?.method ?? "GET"),
        body: String(init?.body ?? ""),
      });
      return new Response(JSON.stringify(response), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const report = await client.bugMonitor.generateAssessmentReport({
        source_kind: "tandem_monitor",
        includeProbeResults: true,
        persistArtifact: true,
        routeDestinationIds: ["audit-webhook"],
        includeRawPayloads: true,
      });
      expect(report.sections?.self_monitoring_boundary?.source_kinds).toContain(
        "tandem_monitor"
      );
      expect(report.markdown_summary).toContain("Security Gap Assessment");
      expect(calls[0]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/security/assessment-report",
        method: "POST",
      });
      expect(JSON.parse(calls[0]?.body ?? "{}")).toEqual({
        source_kind: "tandem_monitor",
        include_probe_results: true,
        persist_artifact: true,
        route_destination_ids: ["audit-webhook"],
        include_raw_payloads: true,
      });
    } finally {
      globalThis.fetch = originalFetch;
    }
  });

  it("mutates destination router config through convenience helpers", async () => {
    const client = new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; method: string; body?: string }> = [];
    const baseConfig: BugMonitorConfigResponse = {
      bug_monitor: {
        enabled: true,
        destinations: [
          {
            destination_id: "legacy-github",
            name: "GitHub",
            kind: "github_issue",
            enabled: true,
          },
          {
            destination_id: "linear-primary",
            name: "Linear",
            kind: "linear_issue",
            enabled: true,
          },
        ],
        routes: [
          {
            route_id: "default",
            name: "Default",
            destination_ids: ["legacy-github"],
          },
          {
            route_id: "high-risk",
            name: "High risk",
            destination_ids: ["linear-primary"],
            match_risk_levels: ["high"],
          },
        ],
        default_destination_ids: ["legacy-github"],
      },
    };
    globalThis.fetch = (async (input, init) => {
      const url = String(input);
      const method = String(init?.method ?? "GET");
      const body = typeof init?.body === "string" ? init.body : undefined;
      calls.push({ url, method, body });
      if (url.endsWith("/bug-monitor/drafts/draft-1/publish")) {
        return new Response(JSON.stringify({ ok: true }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response(body || JSON.stringify(baseConfig), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    const linearDestination: BugMonitorDestinationConfig = {
      destination_id: "linear-primary",
      name: "Linear",
      kind: "linear_issue",
      enabled: true,
    };
    const highRiskRoute: BugMonitorRouteConfig = {
      route_id: "high-risk",
      name: "High risk",
      destination_ids: ["linear-primary"],
      match_risk_levels: ["high"],
    };

    try {
      await client.bugMonitor.listDestinations();
      await client.bugMonitor.upsertDestination(linearDestination);
      await client.bugMonitor.upsertRoute(highRiskRoute);
      await client.bugMonitor.removeDestination("linear-primary");
      await client.bugMonitor.publishDraftToDestinations("draft-1", ["legacy-github"], {
        reason: "ship it",
      });

      expect(calls[0]).toMatchObject({
        url: "http://localhost:39731/config/bug-monitor",
        method: "GET",
      });
      expect(calls[2]).toMatchObject({
        url: "http://localhost:39731/config/bug-monitor",
        method: "PATCH",
      });
      expect(JSON.parse(calls[2]?.body || "{}").bug_monitor.destinations).toContainEqual(
        linearDestination
      );
      expect(JSON.parse(calls[4]?.body || "{}").bug_monitor.routes).toContainEqual(highRiskRoute);
      const removePayload = JSON.parse(calls[6]?.body || "{}").bug_monitor;
      expect(removePayload.destinations).not.toContainEqual(linearDestination);
      expect(removePayload.default_destination_ids).toEqual(["legacy-github"]);
      expect(removePayload.routes).toContainEqual({
        route_id: "default",
        name: "Default",
        destination_ids: ["legacy-github"],
      });
      expect(removePayload.routes).not.toContainEqual(highRiskRoute);
      expect(removePayload.routes).not.toContainEqual({
        ...highRiskRoute,
        destination_ids: [],
      });
      expect(calls[7]).toMatchObject({
        url: "http://localhost:39731/bug-monitor/drafts/draft-1/publish",
        method: "POST",
      });
      expect(calls[7]?.body).toBe(
        JSON.stringify({
          reason: "ship it",
          destination_ids: ["legacy-github"],
        })
      );
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
