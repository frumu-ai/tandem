import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import { Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { useModes } from "@/hooks/useModes";
import { ModeIconPicker } from "@/components/settings/modes/ModeIconPicker";
import {
  deleteMode,
  exportModes,
  importModes,
  mcpListTools,
  upsertMode,
  type McpRemoteTool,
  type ModeBase,
  type ModeDefinition,
  type ModeScope,
} from "@/lib/tauri";

const BASE_MODE_OPTIONS: { value: ModeBase; label: string }[] = [
  { value: "immediate", label: "Immediate" },
  { value: "plan", label: "Plan" },
  { value: "orchestrate", label: "Orchestrate" },
  { value: "coder", label: "Coder" },
  { value: "ask", label: "Ask" },
  { value: "explore", label: "Explore" },
];

interface FormState {
  id: string;
  label: string;
  baseMode: ModeBase;
  icon: string;
  scope: ModeScope;
  systemPromptAppend: string;
  allowedTools: string;
  editGlobs: string;
  autoApprove: boolean;
}

const DEFAULT_FORM: FormState = {
  id: "",
  label: "",
  baseMode: "ask",
  icon: "zap",
  scope: "user",
  systemPromptAppend: "",
  allowedTools: "",
  editGlobs: "",
  autoApprove: false,
};

function modeToForm(mode: ModeDefinition): FormState {
  return {
    id: mode.id,
    label: mode.label,
    baseMode: mode.base_mode,
    icon: mode.icon ?? "zap",
    scope: mode.source === "project" ? "project" : "user",
    systemPromptAppend: mode.system_prompt_append ?? "",
    allowedTools: mode.allowed_tools?.join(", ") ?? "",
    editGlobs: mode.edit_globs?.join("\n") ?? "",
    autoApprove: Boolean(mode.auto_approve),
  };
}

export function ModesSettings() {
  const { modes, isLoading, error, refreshModes } = useModes();
  const [selectedId, setSelectedId] = useState<string>("");
  const [form, setForm] = useState<FormState>(DEFAULT_FORM);
  const [jsonBuffer, setJsonBuffer] = useState<string>("");
  const [importScope, setImportScope] = useState<ModeScope>("user");
  const [status, setStatus] = useState<string>("");
  const [isSaving, setIsSaving] = useState(false);
  const [mcpTools, setMcpTools] = useState<McpRemoteTool[]>([]);

  const editableModes = useMemo(
    () => modes.filter((m) => m.source === "user" || m.source === "project"),
    [modes]
  );

  useEffect(() => {
    if (!selectedId) return;
    const selected = modes.find((m) => m.id === selectedId);
    if (!selected) {
      setSelectedId("");
      setForm(DEFAULT_FORM);
      return;
    }
    setForm(modeToForm(selected));
  }, [modes, selectedId]);

  useEffect(() => {
    let disposed = false;
    const loadMcpTools = async () => {
      try {
        const loaded = await mcpListTools();
        if (!disposed) setMcpTools(loaded);
      } catch {
        if (!disposed) setMcpTools([]);
      }
    };
    void loadMcpTools();
    return () => {
      disposed = true;
    };
  }, []);

  const selectedMode = modes.find((m) => m.id === selectedId);
  const isBuiltInSelected = selectedMode?.source === "builtin";
  const allowedToolSet = useMemo(
    () =>
      new Set(
        form.allowedTools
          .split(",")
          .map((tool) => tool.trim())
          .filter(Boolean)
      ),
    [form.allowedTools]
  );
  const groupedMcpTools = useMemo(() => {
    const grouped = new Map<string, McpRemoteTool[]>();
    for (const tool of mcpTools) {
      if (!grouped.has(tool.server_name)) grouped.set(tool.server_name, []);
      grouped.get(tool.server_name)?.push(tool);
    }
    for (const rows of grouped.values()) {
      rows.sort((a, b) => a.namespaced_name.localeCompare(b.namespaced_name));
    }
    return Array.from(grouped.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [mcpTools]);

  const toggleAllowedTool = (toolName: string, enabled: boolean) => {
    setForm((prev) => {
      const next = new Set(
        prev.allowedTools
          .split(",")
          .map((tool) => tool.trim())
          .filter(Boolean)
      );
      if (enabled) {
        next.add(toolName);
      } else {
        next.delete(toolName);
      }
      return { ...prev, allowedTools: Array.from(next).join(", ") };
    });
  };

  const saveMode = async () => {
    setIsSaving(true);
    setStatus("");
    try {
      const allowedTools = form.allowedTools
        .split(",")
        .map((t) => t.trim())
        .filter(Boolean);
      const editGlobs = form.editGlobs
        .split(/\r?\n/)
        .map((p) => p.trim())
        .filter(Boolean);

      const payload: ModeDefinition = {
        id: form.id.trim(),
        label: form.label.trim(),
        base_mode: form.baseMode,
        icon: form.icon.trim() || undefined,
        system_prompt_append: form.systemPromptAppend.trim() || undefined,
        allowed_tools: allowedTools.length > 0 ? allowedTools : undefined,
        edit_globs: editGlobs.length > 0 ? editGlobs : undefined,
        auto_approve: form.autoApprove || undefined,
      };
      await upsertMode(form.scope, payload);
      await refreshModes();
      setSelectedId(payload.id);
      setStatus(`Saved mode '${payload.id}' to ${form.scope}.`);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : "Failed to save mode");
    } finally {
      setIsSaving(false);
    }
  };

  const removeMode = async () => {
    if (!selectedMode || selectedMode.source === "builtin") return;
    setIsSaving(true);
    setStatus("");
    try {
      const scope: ModeScope = selectedMode.source === "project" ? "project" : "user";
      await deleteMode(scope, selectedMode.id);
      await refreshModes();
      setSelectedId("");
      setForm(DEFAULT_FORM);
      setStatus(`Deleted mode '${selectedMode.id}' from ${scope}.`);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : "Failed to delete mode");
    } finally {
      setIsSaving(false);
    }
  };

  const doExport = async () => {
    setStatus("");
    try {
      const json = await exportModes(importScope);
      setJsonBuffer(json);
      setStatus(`Loaded ${importScope} modes into JSON buffer.`);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : "Failed to export modes");
    }
  };

  const doImport = async () => {
    setIsSaving(true);
    setStatus("");
    try {
      await importModes(importScope, jsonBuffer);
      await refreshModes();
      setStatus(`Imported modes into ${importScope} scope.`);
    } catch (e) {
      setStatus(e instanceof Error ? e.message : "Failed to import modes");
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Custom Modes</CardTitle>
        <CardDescription>
          Define user or project modes with tool restrictions and optional edit path guards.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid gap-2">
          <label className="text-xs font-medium text-text-muted">Existing modes</label>
          <select
            className="rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
            value={selectedId}
            onChange={(e) => setSelectedId(e.target.value)}
          >
            <option value="">Create new mode...</option>
            {modes.map((mode) => (
              <option key={mode.id} value={mode.id}>
                {mode.label} ({mode.source ?? "builtin"})
              </option>
            ))}
          </select>
          <p className="text-xs text-text-subtle">
            {isLoading
              ? "Loading modes..."
              : `Loaded ${modes.length} mode(s), ${editableModes.length} editable.`}
          </p>
          {error && <p className="text-xs text-error">{error}</p>}
        </div>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div>
            <label className="text-xs font-medium text-text-muted">Mode ID</label>
            <Input
              value={form.id}
              disabled={isBuiltInSelected}
              onChange={(e) => setForm((prev) => ({ ...prev, id: e.target.value }))}
              placeholder="safe-coder"
            />
          </div>
          <div>
            <label className="text-xs font-medium text-text-muted">Label</label>
            <Input
              value={form.label}
              onChange={(e) => setForm((prev) => ({ ...prev, label: e.target.value }))}
              placeholder="Safe Coder"
            />
          </div>
          <div>
            <label className="text-xs font-medium text-text-muted">Base mode</label>
            <select
              className="w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
              value={form.baseMode}
              onChange={(e) =>
                setForm((prev) => ({ ...prev, baseMode: e.target.value as ModeBase }))
              }
            >
              {BASE_MODE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="text-xs font-medium text-text-muted">Icon</label>
            <ModeIconPicker
              value={form.icon}
              onChange={(nextIcon) => setForm((prev) => ({ ...prev, icon: nextIcon }))}
            />
          </div>
          <div>
            <label className="text-xs font-medium text-text-muted">Save scope</label>
            <select
              className="w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
              value={form.scope}
              onChange={(e) => setForm((prev) => ({ ...prev, scope: e.target.value as ModeScope }))}
            >
              <option value="user">User</option>
              <option value="project">Project</option>
            </select>
          </div>
        </div>

        <div>
          <label className="text-xs font-medium text-text-muted">System prompt append</label>
          <textarea
            className="mt-1 min-h-[72px] w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
            value={form.systemPromptAppend}
            onChange={(e) => setForm((prev) => ({ ...prev, systemPromptAppend: e.target.value }))}
            placeholder="Extra instructions appended before each user request..."
          />
        </div>

        <div>
          <label className="text-xs font-medium text-text-muted">
            Allowed tools (comma-separated)
          </label>
          <Input
            value={form.allowedTools}
            onChange={(e) => setForm((prev) => ({ ...prev, allowedTools: e.target.value }))}
            placeholder="read, list, todowrite"
          />
        </div>

        {groupedMcpTools.length > 0 && (
          <div className="space-y-2 rounded-md border border-border bg-surface-elevated/40 p-3">
            <p className="text-xs font-medium text-text-muted">Connector tools (MCP)</p>
            {groupedMcpTools.map(([server, tools]) => (
              <div key={server} className="space-y-1">
                <p className="text-[11px] font-semibold text-text">{server}</p>
                <div className="grid grid-cols-1 gap-1 md:grid-cols-2">
                  {tools.map((tool) => (
                    <label
                      key={tool.namespaced_name}
                      className="flex items-center gap-2 rounded border border-border bg-surface px-2 py-1 text-xs"
                    >
                      <input
                        type="checkbox"
                        checked={allowedToolSet.has(tool.namespaced_name)}
                        onChange={(event) =>
                          toggleAllowedTool(tool.namespaced_name, event.target.checked)
                        }
                      />
                      <span className="truncate font-mono" title={tool.namespaced_name}>
                        {tool.namespaced_name}
                      </span>
                    </label>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}

        <div>
          <label className="text-xs font-medium text-text-muted">
            Edit globs (one pattern per line)
          </label>
          <textarea
            className="mt-1 min-h-[72px] w-full rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
            value={form.editGlobs}
            onChange={(e) => setForm((prev) => ({ ...prev, editGlobs: e.target.value }))}
            placeholder="src/**&#10;docs/**"
          />
        </div>

        <div className="flex items-center justify-between rounded-md border border-border bg-surface-elevated px-3 py-2">
          <span className="text-sm text-text">Auto-approve (mode metadata)</span>
          <Switch
            checked={form.autoApprove}
            onChange={(e) => setForm((prev) => ({ ...prev, autoApprove: e.target.checked }))}
          />
        </div>

        <div className="flex flex-wrap gap-2">
          <Button onClick={saveMode} disabled={isSaving || !form.id.trim() || !form.label.trim()}>
            Save Mode
          </Button>
          <Button variant="ghost" onClick={() => setForm(DEFAULT_FORM)} disabled={isSaving}>
            Reset Form
          </Button>
          <Button
            variant="ghost"
            onClick={removeMode}
            disabled={isSaving || !selectedMode || selectedMode.source === "builtin"}
            className="text-error hover:bg-error/10"
          >
            Delete Selected
          </Button>
        </div>

        <div className="space-y-2 rounded-md border border-border bg-surface-elevated/40 p-3">
          <div className="flex items-center gap-2">
            <label className="text-xs font-medium text-text-muted">Import/Export scope</label>
            <select
              className="rounded-md border border-border bg-surface px-2 py-1 text-xs text-text"
              value={importScope}
              onChange={(e) => setImportScope(e.target.value as ModeScope)}
            >
              <option value="user">User</option>
              <option value="project">Project</option>
            </select>
            <Button size="sm" variant="ghost" onClick={doExport} disabled={isSaving}>
              Export to Buffer
            </Button>
            <Button size="sm" onClick={doImport} disabled={isSaving || !jsonBuffer.trim()}>
              Import From Buffer
            </Button>
          </div>
          <textarea
            className="min-h-[120px] w-full rounded-md border border-border bg-surface px-3 py-2 font-mono text-xs text-text"
            value={jsonBuffer}
            onChange={(e) => setJsonBuffer(e.target.value)}
            placeholder='[{"id":"safe-coder","label":"Safe Coder","base_mode":"coder"}]'
          />
        </div>

        {status && <p className="text-xs text-text-muted">{status}</p>}
      </CardContent>
    </Card>
  );
}
