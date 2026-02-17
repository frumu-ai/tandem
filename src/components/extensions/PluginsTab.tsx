import { useEffect, useState } from "react";
import { Plug, Trash2 } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import {
  opencodeAddPlugin,
  opencodeListPlugins,
  opencodeRemovePlugin,
  type OpenCodeConfigScope,
} from "@/lib/tauri";

interface PluginsTabProps {
  workspacePath: string | null;
}

export function PluginsTab({ workspacePath }: PluginsTabProps) {
  const hasWorkspace = !!workspacePath;
  const [scope, setScope] = useState<OpenCodeConfigScope>(hasWorkspace ? "project" : "global");
  const [plugins, setPlugins] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newPlugin, setNewPlugin] = useState("");

  useEffect(() => {
    if (!hasWorkspace && scope === "project") setScope("global");
  }, [hasWorkspace, scope]);

  const refresh = async (nextScope: OpenCodeConfigScope = scope) => {
    setLoading(true);
    try {
      setError(null);
      setPlugins(await opencodeListPlugins(nextScope));
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load plugins");
      setPlugins([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh().catch(console.error);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope, workspacePath]);

  const onAdd = async () => {
    const name = newPlugin.trim();
    if (!name) return;
    setSaving(true);
    try {
      setError(null);
      const updated = await opencodeAddPlugin(scope, name);
      setPlugins(updated);
      setNewPlugin("");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to add plugin");
    } finally {
      setSaving(false);
    }
  };

  const onRemove = async (name: string) => {
    setSaving(true);
    try {
      setError(null);
      const updated = await opencodeRemovePlugin(scope, name);
      setPlugins(updated);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to remove plugin");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-6">
      {error && (
        <div className="rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
          {error}
        </div>
      )}

      {/* Scope selector */}
      <div className="flex items-center justify-between gap-4">
        <div>
          <p className="text-sm font-medium text-text">Scope</p>
          <p className="text-xs text-text-subtle">Choose where plugins are configured</p>
        </div>
        <div className="flex rounded-lg border border-border bg-surface overflow-hidden">
          <button
            type="button"
            onClick={() => setScope("project")}
            disabled={!hasWorkspace}
            className={cn(
              "px-3 py-2 text-sm font-medium transition-colors",
              scope === "project"
                ? "bg-primary/20 text-primary"
                : "text-text-muted hover:bg-surface-elevated hover:text-text",
              !hasWorkspace &&
                "cursor-not-allowed opacity-50 hover:bg-transparent hover:text-text-muted"
            )}
          >
            Folder
          </button>
          <button
            type="button"
            onClick={() => setScope("global")}
            className={cn(
              "px-3 py-2 text-sm font-medium transition-colors",
              scope === "global"
                ? "bg-primary/20 text-primary"
                : "text-text-muted hover:bg-surface-elevated hover:text-text"
            )}
          >
            Global
          </button>
        </div>
      </div>

      <Card>
        <CardHeader>
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1">
              <CardTitle>Configured plugins</CardTitle>
              <CardDescription>
                Tandem edits engine plugin config only. Restart the engine to apply changes.
              </CardDescription>
            </div>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => openUrl("https://opencode.ai/docs/plugins")}
            >
              Docs
            </Button>
          </div>
        </CardHeader>
        <CardContent className="space-y-3">
          {loading ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              Loading plugins...
            </div>
          ) : plugins.length === 0 ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-6 text-center">
              <Plug className="mx-auto mb-2 h-8 w-8 text-text-subtle" />
              <p className="text-sm text-text-muted">No plugins configured.</p>
              <p className="text-xs text-text-subtle">Add one below to enable it in Tandem.</p>
            </div>
          ) : (
            <div className="space-y-2">
              {plugins.map((p) => (
                <div
                  key={p}
                  className="flex items-center justify-between gap-3 rounded-lg border border-border bg-surface-elevated p-3"
                >
                  <p className="min-w-0 truncate font-mono text-sm text-text">{p}</p>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => onRemove(p)}
                    disabled={saving}
                    className="text-text-subtle hover:text-error hover:bg-error/10"
                    title="Remove"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Add plugin</CardTitle>
          <CardDescription>
            Enter a plugin identifier (usually an npm package name).
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <Input
            value={newPlugin}
            onChange={(e) => setNewPlugin(e.target.value)}
            placeholder="@scope/plugin-name"
            onKeyDown={(e) => {
              if (e.key === "Enter") onAdd();
            }}
          />
          <div className="flex items-center justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                setNewPlugin("");
                setError(null);
              }}
              disabled={saving}
            >
              Clear
            </Button>
            <Button onClick={onAdd} disabled={saving || !newPlugin.trim()}>
              {saving ? "Saving..." : "Add"}
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
