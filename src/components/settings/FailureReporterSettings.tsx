import { useCallback, useEffect, useMemo, useState } from "react";
import { AlertCircle, Bug, ExternalLink, RefreshCw, Siren } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import {
  getFailureReporterConfig,
  getFailureReporterStatus,
  listFailureReporterDrafts,
  mcpListServers,
  patchFailureReporterConfig,
  type FailureReporterConfig,
  type FailureReporterDraftRecord,
  type FailureReporterStatus,
  type McpServerRecord,
} from "@/lib/tauri";

interface FailureReporterSettingsProps {
  providerCatalogModels: Record<string, string[]>;
  onOpenMcpSettings?: () => void;
}

function emptyConfig(): FailureReporterConfig {
  return {
    enabled: false,
    repo: "",
    mcp_server: "",
    provider_preference: "auto",
    model_policy: null,
    require_approval_for_new_issues: true,
    auto_comment_on_matched_open_issues: true,
  };
}

function normalizeConfig(config?: FailureReporterConfig | null): FailureReporterConfig {
  if (!config) return emptyConfig();
  return {
    enabled: !!config.enabled,
    repo: config.repo ?? "",
    mcp_server: config.mcp_server ?? "",
    provider_preference: config.provider_preference ?? "auto",
    model_policy: config.model_policy ?? null,
    require_approval_for_new_issues: config.require_approval_for_new_issues ?? true,
    auto_comment_on_matched_open_issues: config.auto_comment_on_matched_open_issues ?? true,
    updated_at_ms: config.updated_at_ms ?? 0,
  };
}

export function FailureReporterSettings({
  providerCatalogModels,
  onOpenMcpSettings,
}: FailureReporterSettingsProps) {
  const [config, setConfig] = useState<FailureReporterConfig>(emptyConfig);
  const [status, setStatus] = useState<FailureReporterStatus | null>(null);
  const [drafts, setDrafts] = useState<FailureReporterDraftRecord[]>([]);
  const [mcpServers, setMcpServers] = useState<McpServerRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const providerId = String(config.model_policy?.default_model?.provider_id ?? "").trim();
  const modelId = String(config.model_policy?.default_model?.model_id ?? "").trim();
  const providerOptions = useMemo(
    () => Object.keys(providerCatalogModels).sort((a, b) => a.localeCompare(b)),
    [providerCatalogModels]
  );
  const modelOptions = providerId ? (providerCatalogModels[providerId] ?? []) : [];

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [configPayload, statusPayload, draftsPayload, servers] = await Promise.all([
        getFailureReporterConfig(),
        getFailureReporterStatus(),
        listFailureReporterDrafts(20),
        mcpListServers(),
      ]);
      setConfig(normalizeConfig(configPayload.failure_reporter));
      setStatus(statusPayload.status ?? null);
      setDrafts(Array.isArray(draftsPayload.drafts) ? draftsPayload.drafts : []);
      setMcpServers(servers);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load Failure Reporter settings");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const updateConfig = (patch: Partial<FailureReporterConfig>) => {
    setConfig((prev) => ({ ...prev, ...patch }));
  };

  const setModelRoute = (nextProviderId: string, nextModelId: string) => {
    const provider = nextProviderId.trim();
    const model = nextModelId.trim();
    updateConfig({
      model_policy:
        provider && model
          ? {
              default_model: {
                provider_id: provider,
                model_id: model,
              },
            }
          : null,
    });
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const repo = String(config.repo ?? "").trim();
      const mcpServer = String(config.mcp_server ?? "").trim();
      const payload = {
        enabled: !!config.enabled,
        repo: repo || null,
        mcp_server: mcpServer || null,
        provider_preference: config.provider_preference || "auto",
        model_policy:
          providerId && modelId
            ? {
                default_model: {
                  provider_id: providerId,
                  model_id: modelId,
                },
              }
            : null,
        require_approval_for_new_issues: config.require_approval_for_new_issues ?? true,
        auto_comment_on_matched_open_issues: config.auto_comment_on_matched_open_issues ?? true,
      };
      const response = await patchFailureReporterConfig(payload);
      setConfig(normalizeConfig(response.failure_reporter));
      setNotice("Failure Reporter settings saved.");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save Failure Reporter settings");
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Siren className="h-5 w-5 text-primary" />
              Failure Reporter
            </CardTitle>
            <CardDescription>
              Configure engine-backed failure-to-issue reporting and inspect pending drafts without
              leaving desktop.
            </CardDescription>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="ghost" onClick={() => void refresh()} disabled={loading}>
              <RefreshCw className="mr-2 h-4 w-4" />
              Reload
            </Button>
            {onOpenMcpSettings && (
              <Button size="sm" variant="secondary" onClick={onOpenMcpSettings}>
                <ExternalLink className="mr-2 h-4 w-4" />
                Open MCP Setup
              </Button>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {error && (
          <div className="rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
            {error}
          </div>
        )}
        {notice && (
          <div className="rounded-lg border border-success/20 bg-success/10 p-3 text-sm text-success">
            {notice}
          </div>
        )}

        <div className="grid gap-4 md:grid-cols-2">
          <div className="rounded-xl border border-border bg-surface-elevated/40 p-4">
            <div className="mb-2 flex items-center justify-between gap-3">
              <div>
                <p className="text-sm font-medium text-text">Reporter state</p>
                <p className="text-xs text-text-muted">
                  For v1, use this for known Tandem failures and manual reporting paths.
                </p>
              </div>
              <Switch
                checked={!!config.enabled}
                onChange={(event) => updateConfig({ enabled: event.target.checked })}
              />
            </div>
            <p className="text-xs text-text-subtle">
              {status?.readiness?.runtime_ready ? "Ready" : "Blocked"}{" "}
              {status?.last_error ? `- ${status.last_error}` : ""}
            </p>
          </div>

          <div className="rounded-xl border border-border bg-surface-elevated/40 p-4">
            <p className="text-sm font-medium text-text">Pending drafts</p>
            <p className="mt-1 text-2xl font-semibold text-text">
              {Number(
                status?.pending_drafts ??
                  drafts.filter((row) => row.status === "approval_required").length
              )}
            </p>
            <p className="text-xs text-text-subtle">
              Drafts are generated by the engine and should later feed coder triage runs.
            </p>
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-2">
          <div className="space-y-2">
            <label className="text-xs font-medium text-text-subtle">Target repo</label>
            <Input
              placeholder="owner/repo"
              value={String(config.repo ?? "")}
              onChange={(event) => updateConfig({ repo: event.target.value })}
            />
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-text-subtle">MCP server</label>
            <select
              className="w-full rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text focus:outline-none focus:ring-2 focus:ring-primary/40"
              value={String(config.mcp_server ?? "")}
              onChange={(event) => updateConfig({ mcp_server: event.target.value })}
            >
              <option value="">Select an MCP server</option>
              {mcpServers.map((server) => (
                <option key={server.name} value={server.name}>
                  {server.name}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-text-subtle">Provider preference</label>
            <select
              className="w-full rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text focus:outline-none focus:ring-2 focus:ring-primary/40"
              value={String(config.provider_preference ?? "auto")}
              onChange={(event) =>
                updateConfig({
                  provider_preference: event.target
                    .value as FailureReporterConfig["provider_preference"],
                })
              }
            >
              <option value="auto">Auto</option>
              <option value="official_github">Official GitHub</option>
              <option value="composio">Composio</option>
              <option value="arcade">Arcade</option>
            </select>
          </div>
          <div className="space-y-2">
            <label className="text-xs font-medium text-text-subtle">Provider</label>
            <select
              className="w-full rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text focus:outline-none focus:ring-2 focus:ring-primary/40"
              value={providerId}
              onChange={(event) => setModelRoute(event.target.value, "")}
            >
              <option value="">Select a provider</option>
              {providerOptions.map((provider) => (
                <option key={provider} value={provider}>
                  {provider}
                </option>
              ))}
            </select>
          </div>
          <div className="space-y-2 md:col-span-2">
            <label className="text-xs font-medium text-text-subtle">Model</label>
            <Input
              list="failure-reporter-model-options"
              value={modelId}
              onChange={(event) => setModelRoute(providerId, event.target.value)}
              placeholder={providerId ? "Type or paste a model id" : "Choose a provider first"}
              disabled={!providerId}
            />
            <datalist id="failure-reporter-model-options">
              {modelOptions.map((option) => (
                <option key={option} value={option} />
              ))}
            </datalist>
            <p className="text-xs text-text-subtle">
              {providerId
                ? modelOptions.length
                  ? `${modelOptions.length} suggested models available for ${providerId}.`
                  : "No catalog models available. Manual model ids are allowed."
                : "Select a provider to load model suggestions."}
            </p>
          </div>
        </div>

        <div className="grid gap-4 md:grid-cols-2">
          <div className="rounded-xl border border-border bg-surface-elevated/40 p-4">
            <p className="text-sm font-medium text-text">Capability readiness</p>
            <div className="mt-2 space-y-1 text-xs text-text-muted">
              <div>
                github.list_issues:{" "}
                {status?.required_capabilities?.github_list_issues ? "ready" : "missing"}
              </div>
              <div>
                github.get_issue:{" "}
                {status?.required_capabilities?.github_get_issue ? "ready" : "missing"}
              </div>
              <div>
                github.create_issue:{" "}
                {status?.required_capabilities?.github_create_issue ? "ready" : "missing"}
              </div>
              <div>
                github.comment_on_issue:{" "}
                {status?.required_capabilities?.github_comment_on_issue ? "ready" : "missing"}
              </div>
            </div>
            {status?.missing_required_capabilities?.length ? (
              <p className="mt-3 text-xs text-error">
                Missing: {status.missing_required_capabilities.join(", ")}
              </p>
            ) : null}
          </div>

          <div className="rounded-xl border border-border bg-surface-elevated/40 p-4">
            <p className="text-sm font-medium text-text">Current model route</p>
            <p className="mt-2 text-sm text-text">
              {status?.selected_model?.provider_id && status?.selected_model?.model_id
                ? `${status.selected_model.provider_id} / ${status.selected_model.model_id}`
                : "No dedicated model selected"}
            </p>
            <p className="mt-1 text-xs text-text-subtle">
              {status?.readiness?.selected_model_ready
                ? "Selected model is available."
                : "Reporter fails closed when the selected model is unavailable."}
            </p>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-3">
          <Button onClick={() => void save()} disabled={saving || loading}>
            {saving ? "Saving..." : "Save Failure Reporter"}
          </Button>
          {onOpenMcpSettings && (
            <Button variant="secondary" onClick={onOpenMcpSettings}>
              Configure GitHub MCP
            </Button>
          )}
        </div>

        <div className="space-y-3 rounded-xl border border-border bg-surface-elevated/40 p-4">
          <div className="flex items-center gap-2">
            <Bug className="h-4 w-4 text-primary" />
            <p className="text-sm font-medium text-text">Recent drafts</p>
          </div>
          {loading ? (
            <p className="text-sm text-text-muted">Loading drafts...</p>
          ) : drafts.length === 0 ? (
            <div className="flex items-start gap-2 rounded-lg border border-border bg-surface p-3 text-sm text-text-muted">
              <AlertCircle className="mt-0.5 h-4 w-4 flex-shrink-0" />
              No Failure Reporter drafts yet.
            </div>
          ) : (
            <div className="space-y-2">
              {drafts.map((draft) => (
                <div
                  key={draft.draft_id}
                  className="rounded-lg border border-border bg-surface p-3"
                >
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="font-medium text-text">{draft.title || draft.fingerprint}</div>
                    <span className="rounded-full border border-border px-2 py-0.5 text-xs text-text-muted">
                      {draft.status}
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-text-subtle">
                    {draft.repo}
                    {draft.issue_number ? ` · issue #${draft.issue_number}` : " · draft only"}
                  </p>
                  {draft.detail ? (
                    <p className="mt-2 text-sm text-text-muted">{draft.detail}</p>
                  ) : null}
                </div>
              ))}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
