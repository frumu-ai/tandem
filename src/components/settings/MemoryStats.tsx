import { useState, useEffect } from "react";
import {
  getMemoryStats,
  indexWorkspace,
  getAppState,
  type MemoryStats as MemoryStatsType,
} from "@/lib/tauri";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/Card";
import { Database, RefreshCw, Play } from "lucide-react";
import { Button } from "@/components/ui/Button";

interface IndexingProgress {
  files_processed: number;
  current_file: string;
}

export function MemoryStats() {
  const [stats, setStats] = useState<MemoryStatsType | null>(null);
  const [loading, setLoading] = useState(false);
  const [indexing, setIndexing] = useState(false);
  const [progress, setProgress] = useState<IndexingProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadStats = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getMemoryStats();
      setStats(data);
    } catch (err) {
      console.error("Failed to load memory stats:", err);
      setError("Failed to load memory statistics");
    } finally {
      setLoading(false);
    }
  };

  const handleIndex = async () => {
    setIndexing(true);
    setError(null);
    setProgress(null);
    let unlisten: UnlistenFn | undefined;

    try {
      unlisten = await listen<IndexingProgress>("indexing-progress", (event) => {
        setProgress(event.payload);
      });

      const appState = await getAppState();
      const projectId = appState.active_project_id || "default";
      await indexWorkspace(projectId);
      await loadStats();
    } catch (err) {
      console.error("Failed to index:", err);
      setError("Failed to index workspace");
    } finally {
      if (unlisten) unlisten();
      setIndexing(false);
      setProgress(null);
    }
  };

  useEffect(() => {
    loadStats();
  }, []);

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
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="secondary"
            size="sm"
            onClick={handleIndex}
            disabled={indexing || loading}
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
        {indexing && progress && (
          <div className="mb-4 p-3 bg-slate-50 rounded-md border border-slate-100">
            <div className="flex justify-between text-xs text-slate-500 mb-1">
              <span>Indexing workspace...</span>
              <span>{progress.files_processed} files processed</span>
            </div>
            <div
              className="text-xs font-mono text-slate-700 truncate"
              title={progress.current_file}
            >
              {progress.current_file}
            </div>
          </div>
        )}
        {error ? (
          <div className="text-sm text-red-500">{error}</div>
        ) : !stats ? (
          <div className="text-sm text-slate-500">Loading...</div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <div className="text-sm text-slate-500">Total Chunks</div>
                <div className="font-medium">{stats.total_chunks.toLocaleString()}</div>
              </div>
              <div className="flex items-center justify-between">
                <div className="text-sm text-slate-500">Total Size</div>
                <div className="font-medium">{formatBytes(stats.total_bytes)}</div>
              </div>
              <div className="flex items-center justify-between">
                <div className="text-sm text-slate-500">DB File Size</div>
                <div className="font-medium">{formatBytes(stats.file_size)}</div>
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
                  {stats.session_chunks.toLocaleString()} ({formatBytes(stats.session_bytes)})
                </span>
              </div>

              <div className="flex items-center justify-between text-sm">
                <span className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-green-500"></span>
                  Project
                </span>
                <span className="text-slate-600">
                  {stats.project_chunks.toLocaleString()} ({formatBytes(stats.project_bytes)})
                </span>
              </div>

              <div className="flex items-center justify-between text-sm">
                <span className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-purple-500"></span>
                  Global
                </span>
                <span className="text-slate-600">
                  {stats.global_chunks.toLocaleString()} ({formatBytes(stats.global_bytes)})
                </span>
              </div>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
