import { useState, useEffect } from "react";
import {
  getMemoryStats,
  getAppState,
  getMemorySettings,
  setMemorySettings,
  getProjectMemoryStats,
  type MemoryStats as MemoryStatsType,
  type ProjectMemoryStats,
  type MemorySettings,
} from "@/lib/tauri";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/Card";
import { Database, RefreshCw, Play } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Switch } from "@/components/ui/Switch";
import { useMemoryIndexing } from "@/contexts/MemoryIndexingContext";

export function MemoryStats() {
  const { projects, startIndex, clearFileIndex } = useMemoryIndexing();

  const [scope, setScope] = useState<"all" | "project">("all");
  const [statsAll, setStatsAll] = useState<MemoryStatsType | null>(null);
  const [statsProject, setStatsProject] = useState<ProjectMemoryStats | null>(null);
  const [memorySettings, setMemorySettingsState] = useState<MemorySettings | null>(null);
  const [appState, setAppState] = useState<Awaited<ReturnType<typeof getAppState>> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadStats = async () => {
    setLoading(true);
    setError(null);
    try {
      const [state, settings] = await Promise.all([getAppState(), getMemorySettings()]);
      setAppState(state);
      setMemorySettingsState(settings);

      if (scope === "all") {
        const data = await getMemoryStats();
        setStatsAll(data);
        setStatsProject(null);
      } else {
        const projectId = state.active_project_id || (state.has_workspace ? "default" : null);
        if (!projectId) {
          setStatsProject(null);
          setStatsAll(null);
        } else {
          const data = await getProjectMemoryStats(projectId);
          setStatsProject(data);
          setStatsAll(null);
        }
      }
    } catch (err) {
      console.error("Failed to load memory stats:", err);
      setError("Failed to load memory statistics");
    } finally {
      setLoading(false);
    }
  };

  const handleIndex = async () => {
    setError(null);
    try {
      const state = appState ?? (await getAppState());
      const projectId = state.active_project_id || (state.has_workspace ? "default" : null);
      if (!projectId) {
        setError("No active project selected");
        return;
      }

      await startIndex(projectId);
      await loadStats();
    } catch (err) {
      console.error("Failed to index:", err);
      setError("Failed to index workspace");
    }
  };

  useEffect(() => {
    loadStats();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scope]);

  const activeProjectId =
    appState?.active_project_id || (appState?.has_workspace ? "default" : null);
  const activeProject =
    activeProjectId && appState?.user_projects
      ? (appState.user_projects.find((p) => p.id === activeProjectId) ?? null)
      : null;

  const indexingState = activeProjectId ? projects[activeProjectId] : undefined;
  const progress = indexingState?.progress;
  const indexing = indexingState?.status === "indexing";

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  return (
    <Card className="mt-6">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="space-y-1">
          <CardTitle className="text-base font-medium flex items-center gap-2">
            <Database className="h-4 w-4" />
            Vector Database Stats
          </CardTitle>
          <div className="text-xs text-slate-500">
            {scope === "all"
              ? "Scope: All projects (this device)"
              : `Scope: Active project${activeProject ? ` (${activeProject.name})` : ""}`}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={handleIndex}
            disabled={indexing || loading || scope !== "project" || !activeProjectId}
            className="h-8"
          >
            <Play className={`h-4 w-4 mr-2 ${indexing ? "animate-spin" : ""}`} />
            {indexing ? "Indexing..." : "Index Files"}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={loadStats}
            disabled={loading}
            className="h-8 w-8 p-0"
          >
            <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <div className="mb-4 flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Button
                variant={scope === "all" ? "secondary" : "ghost"}
                size="sm"
                className="h-8"
                onClick={() => setScope("all")}
              >
                All Projects
              </Button>
              <Button
                variant={scope === "project" ? "secondary" : "ghost"}
                size="sm"
                className="h-8"
                onClick={() => setScope("project")}
              >
                Active Project
              </Button>
            </div>

            <Switch
              checked={!!memorySettings?.auto_index_on_project_load}
              disabled={!memorySettings}
              label="Auto-index on project load"
              onChange={async (e) => {
                const next = e.target.checked;
                try {
                  const nextSettings: MemorySettings = { auto_index_on_project_load: next };
                  setMemorySettingsState(nextSettings);
                  await setMemorySettings(nextSettings);
                } catch (err) {
                  console.error("Failed to save memory settings:", err);
                  setError("Failed to save memory settings");
                }
              }}
            />
          </div>

          {memorySettings?.embedding_status && (
            <div className="text-xs text-slate-500">
              Embeddings: {memorySettings.embedding_status}
              {memorySettings.embedding_reason ? ` (${memorySettings.embedding_reason})` : ""}
            </div>
          )}

          {scope === "project" && activeProject && (
            <div className="text-xs text-slate-500 truncate" title={activeProject.path}>
              Active folder: {activeProject.path}
            </div>
          )}
        </div>

        {indexing && progress && (
          <div className="mb-4 p-3 bg-slate-50 rounded-md border border-slate-100">
            <div className="flex justify-between text-xs text-slate-500 mb-2">
              <span>Indexing workspace...</span>
              <span>
                {progress.files_processed}/{progress.total_files} (
                {progress.total_files > 0
                  ? Math.min(
                      100,
                      Math.round((progress.files_processed / progress.total_files) * 100)
                    )
                  : 0}
                %)
              </span>
            </div>
            <div className="h-2 w-full rounded bg-slate-200 overflow-hidden mb-2">
              <div
                className="h-full bg-slate-700 transition-all"
                style={{
                  width: `${
                    progress.total_files > 0
                      ? Math.min(100, (progress.files_processed / progress.total_files) * 100)
                      : 0
                  }%`,
                }}
              />
            </div>
            <div
              className="text-xs font-mono text-slate-700 truncate"
              title={progress.current_file}
            >
              {progress.current_file}
            </div>
            <div className="mt-2 text-xs text-slate-500 flex flex-wrap gap-x-4 gap-y-1">
              <span>Indexed: {progress.indexed_files}</span>
              <span>Skipped: {progress.skipped_files}</span>
              <span>Errors: {progress.errors}</span>
              <span>Chunks: {progress.chunks_created}</span>
            </div>
          </div>
        )}
        {error ? (
          <div className="text-sm text-red-500">{error}</div>
        ) : scope === "all" && !statsAll ? (
          <div className="text-sm text-slate-500">Loading...</div>
        ) : scope === "project" && !activeProjectId ? (
          <div className="text-sm text-slate-500">No workspace selected.</div>
        ) : scope === "project" && !statsProject ? (
          <div className="text-sm text-slate-500">Loading...</div>
        ) : scope === "all" && statsAll ? (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <div className="text-sm text-slate-500">Total Chunks</div>
                <div className="font-medium">{statsAll.total_chunks.toLocaleString()}</div>
              </div>
              <div className="flex items-center justify-between">
                <div className="text-sm text-slate-500">Total Size</div>
                <div className="font-medium">{formatBytes(statsAll.total_bytes)}</div>
              </div>
              <div className="flex items-center justify-between">
                <div className="text-sm text-slate-500">DB File Size</div>
                <div className="font-medium">{formatBytes(statsAll.file_size)}</div>
              </div>
            </div>

            <div className="space-y-2 border-t md:border-t-0 md:border-l border-slate-200 pt-4 md:pt-0 md:pl-4">
              <div className="text-xs font-medium text-slate-500 uppercase mb-2">Breakdown</div>

              <div className="flex items-center justify-between text-sm">
                <span className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-blue-500"></span>
                  Session
                </span>
                <span className="text-slate-600">
                  {statsAll.session_chunks.toLocaleString()} ({formatBytes(statsAll.session_bytes)})
                </span>
              </div>

              <div className="flex items-center justify-between text-sm">
                <span className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-green-500"></span>
                  Project
                </span>
                <span className="text-slate-600">
                  {statsAll.project_chunks.toLocaleString()} ({formatBytes(statsAll.project_bytes)})
                </span>
              </div>

              <div className="flex items-center justify-between text-sm">
                <span className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-purple-500"></span>
                  Global
                </span>
                <span className="text-slate-600">
                  {statsAll.global_chunks.toLocaleString()} ({formatBytes(statsAll.global_bytes)})
                </span>
              </div>
            </div>
          </div>
        ) : (
          <div className="space-y-4">
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <div className="text-sm text-slate-500">Project Chunks</div>
                  <div className="font-medium">{statsProject!.project_chunks.toLocaleString()}</div>
                </div>
                <div className="flex items-center justify-between">
                  <div className="text-sm text-slate-500">Project Size</div>
                  <div className="font-medium">{formatBytes(statsProject!.project_bytes)}</div>
                </div>
                <div className="flex items-center justify-between">
                  <div className="text-sm text-slate-500">Indexed Files</div>
                  <div className="font-medium">{statsProject!.indexed_files.toLocaleString()}</div>
                </div>
              </div>
              <div className="space-y-3 border-t md:border-t-0 md:border-l border-slate-200 pt-4 md:pt-0 md:pl-4">
                <div className="text-xs font-medium text-slate-500 uppercase mb-2">File Index</div>
                <div className="flex items-center justify-between text-sm">
                  <span className="text-slate-600">Chunks</span>
                  <span className="font-medium">
                    {statsProject!.file_index_chunks.toLocaleString()}
                  </span>
                </div>
                <div className="flex items-center justify-between text-sm">
                  <span className="text-slate-600">Size</span>
                  <span className="font-medium">{formatBytes(statsProject!.file_index_bytes)}</span>
                </div>
                <div className="flex items-center justify-between text-sm">
                  <span className="text-slate-600">Last Indexed</span>
                  <span className="font-medium">
                    {statsProject!.last_indexed_at
                      ? new Date(statsProject!.last_indexed_at).toLocaleString()
                      : "Never"}
                  </span>
                </div>
              </div>
            </div>

            <div className="flex flex-col md:flex-row gap-2">
              <Button
                variant="ghost"
                disabled={!activeProjectId}
                onClick={async () => {
                  if (!activeProjectId) return;
                  const ok = window.confirm(
                    "Clear this project's file index? This removes only file-derived vectors and can free up space."
                  );
                  if (!ok) return;
                  const vacuum = window.confirm(
                    "Vacuum database after clearing? This can take longer but reclaims disk space."
                  );
                  try {
                    await clearFileIndex(activeProjectId, vacuum);
                    await loadStats();
                  } catch (err) {
                    console.error("Failed to clear file index:", err);
                    setError("Failed to clear file index");
                  }
                }}
              >
                Clear File Index
              </Button>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
