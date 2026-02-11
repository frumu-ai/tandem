import { useState, useEffect, useCallback, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  Folder,
  FolderOpen,
  FileText,
  FileCode,
  FileJson,
  Image as ImageIcon,
  File,
  Search,
  ChevronRight,
  ChevronDown,
  Loader2,
} from "lucide-react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  readDirectory,
  onSidecarEvent,
  startFileTreeWatcher,
  stopFileTreeWatcher,
  type FileEntry,
  type FileTreeChangedPayload,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface FileBrowserProps {
  rootPath: string | null;
  onFileSelect: (file: FileEntry) => void;
  selectedPath?: string;
}

interface TreeNode extends FileEntry {
  children?: TreeNode[];
  isExpanded?: boolean;
  isLoading?: boolean;
}

export function FileBrowser({ rootPath, onFileSelect, selectedPath }: FileBrowserProps) {
  const [tree, setTree] = useState<TreeNode[]>([]);
  const treeRef = useRef<TreeNode[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refreshTimerRef = useRef<ReturnType<typeof globalThis.setTimeout> | null>(null);

  // Load root directory
  const loadDirectory = useCallback(async (path: string) => {
    try {
      setIsLoading(true);
      setError(null);
      const entries = await readDirectory(path);
      const nodes: TreeNode[] = entries.map((entry) => ({
        ...entry,
        children: entry.is_directory ? [] : undefined,
        isExpanded: false,
      }));
      setTree(nodes);
    } catch (err) {
      console.error("Failed to load directory:", err);
      setError(err instanceof Error ? err.message : "Failed to load directory");
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    if (rootPath) {
      loadDirectory(rootPath);
    }
  }, [rootPath, loadDirectory]);

  useEffect(() => {
    treeRef.current = tree;
  }, [tree]);

  const refreshTree = useCallback(async (path: string) => {
    // Rebuild the tree, refreshing any expanded folders, so new files appear without losing context.
    const build = async (dirPath: string, prevChildren?: TreeNode[]): Promise<TreeNode[]> => {
      const entries = await readDirectory(dirPath);
      const prevByPath = new Map((prevChildren ?? []).map((n) => [n.path, n]));

      const nodes: TreeNode[] = [];
      for (const entry of entries) {
        const prev = prevByPath.get(entry.path);
        const isExpanded = prev?.isExpanded ?? false;
        const node: TreeNode = {
          ...entry,
          children: entry.is_directory ? [] : undefined,
          isExpanded,
          isLoading: false,
        };

        if (entry.is_directory && isExpanded) {
          try {
            node.children = await build(entry.path, prev?.children);
          } catch {
            // If a directory becomes unreadable, keep existing children.
            node.children = prev?.children ?? [];
          }
        }

        nodes.push(node);
      }
      return nodes;
    };

    try {
      const next = await build(path, treeRef.current);
      setTree(next);
    } catch (e) {
      console.warn("[FileBrowser] Failed to refresh tree:", e);
    }
  }, []);

  // Watch for filesystem changes while the Files view is mounted.
  useEffect(() => {
    if (!rootPath) return;

    let disposed = false;
    let unlisten: (() => void) | null = null;

    const normalizeRoot = (p: string) => {
      // Windows can surface verbatim paths (\\?\C:\...) in Rust logs and sometimes via APIs.
      // Also, path case and slashes can differ; normalize so we don't accidentally ignore events.
      let s = p;
      if (s.startsWith("\\\\?\\")) s = s.slice("\\\\?\\".length);
      return s.replace(/\\/g, "/").toLowerCase();
    };

    const scheduleRefresh = () => {
      if (refreshTimerRef.current) {
        globalThis.clearTimeout(refreshTimerRef.current);
      }
      refreshTimerRef.current = globalThis.setTimeout(() => {
        if (disposed) return;
        void refreshTree(rootPath);
      }, 200);
    };

    const start = async () => {
      try {
        const webview = getCurrentWebviewWindow();
        const label = webview.label;
        await startFileTreeWatcher(label, rootPath);
      } catch (e) {
        console.warn("[FileBrowser] Failed to start watcher:", e);
      }

      try {
        // Use the webview-specific listener so we catch events emitted from Rust using
        // WebviewWindow/AppHandle emit variants across Tauri v2 target semantics.
        const webview = getCurrentWebviewWindow();
        const un = await webview.listen<FileTreeChangedPayload>("file-tree-changed", (event) => {
          const payload = event.payload;
          if (!payload?.root) return;
          // Only react to our active root (normalized, tolerant for Windows path formatting).
          // If this ever mismatches unexpectedly, we still refresh as a safe fallback since
          // the Files view is mounted only for a single workspace.
          const matchesRoot = normalizeRoot(payload.root) === normalizeRoot(rootPath);
          if (!matchesRoot) {
            // Fallback: refresh anyway, but keep the console noise low.
            // eslint-disable-next-line no-console
            console.debug?.("[FileBrowser] file-tree-changed root mismatch; refreshing anyway", {
              payloadRoot: payload.root,
              rootPath,
            });
          }
          scheduleRefresh();
        });
        unlisten = un;
      } catch (e) {
        console.warn("[FileBrowser] Failed to listen for file-tree-changed:", e);
      }
    };

    void start();

    return () => {
      disposed = true;
      if (refreshTimerRef.current) {
        globalThis.clearTimeout(refreshTimerRef.current);
      }
      if (unlisten) unlisten();
      // Best-effort stop; if another view starts it immediately, that's fine.
      void stopFileTreeWatcher();
    };
  }, [rootPath, refreshTree]);

  // Refresh on file-writing tool events from the sidecar (AI-created files).
  // This is a reliable fallback even if OS filesystem notifications are flaky.
  useEffect(() => {
    if (!rootPath) return;

    let unlisten: (() => void) | null = null;
    let disposed = false;

    const isFileTool = (tool: string) =>
      tool === "write" ||
      tool === "write_file" ||
      tool === "create_file" ||
      tool === "delete" ||
      tool === "delete_file";

    const scheduleRefresh = () => {
      if (refreshTimerRef.current) {
        globalThis.clearTimeout(refreshTimerRef.current);
      }
      refreshTimerRef.current = globalThis.setTimeout(() => {
        if (disposed) return;
        void refreshTree(rootPath);
      }, 150);
    };

    const start = async () => {
      try {
        unlisten = await onSidecarEvent((event) => {
          if (event.type === "file_edited") {
            scheduleRefresh();
            return;
          }
          if (event.type === "tool_end" && isFileTool(event.tool)) {
            scheduleRefresh();
            return;
          }
        });
      } catch (e) {
        console.warn("[FileBrowser] Failed to listen for sidecar file events:", e);
      }
    };

    void start();

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [rootPath, refreshTree]);

  // Load children for a directory
  const loadChildren = async (node: TreeNode, path: string[]): Promise<TreeNode[]> => {
    if (!node.is_directory) return tree;

    try {
      const entries = await readDirectory(node.path);
      const children: TreeNode[] = entries.map((entry) => ({
        ...entry,
        children: entry.is_directory ? [] : undefined,
        isExpanded: false,
      }));

      // Update tree immutably
      const updateNode = (nodes: TreeNode[], currentPath: string[]): TreeNode[] => {
        if (currentPath.length === 0) {
          return nodes.map((n) =>
            n.path === node.path ? { ...n, children, isExpanded: true, isLoading: false } : n
          );
        }

        const [first, ...rest] = currentPath;
        return nodes.map((n) =>
          n.name === first && n.children ? { ...n, children: updateNode(n.children, rest) } : n
        );
      };

      return updateNode(tree, path);
    } catch (err) {
      console.error("Failed to load children:", err);
      return tree;
    }
  };

  // Toggle directory expansion
  const toggleDirectory = async (node: TreeNode, path: string[]) => {
    if (!node.is_directory) {
      onFileSelect(node);
      return;
    }

    if (node.isExpanded) {
      // Collapse
      const collapseNode = (nodes: TreeNode[], currentPath: string[]): TreeNode[] => {
        if (currentPath.length === 0) {
          return nodes.map((n) => (n.path === node.path ? { ...n, isExpanded: false } : n));
        }

        const [first, ...rest] = currentPath;
        return nodes.map((n) =>
          n.name === first && n.children ? { ...n, children: collapseNode(n.children, rest) } : n
        );
      };

      setTree(collapseNode(tree, path));
    } else {
      // Expand - mark as loading first
      const markLoading = (nodes: TreeNode[], currentPath: string[]): TreeNode[] => {
        if (currentPath.length === 0) {
          return nodes.map((n) => (n.path === node.path ? { ...n, isLoading: true } : n));
        }

        const [first, ...rest] = currentPath;
        return nodes.map((n) =>
          n.name === first && n.children ? { ...n, children: markLoading(n.children, rest) } : n
        );
      };

      setTree(markLoading(tree, path));
      const newTree = await loadChildren(node, path);
      setTree(newTree);
    }
  };

  // Filter tree by search query
  const filterTree = (nodes: TreeNode[], query: string): TreeNode[] => {
    if (!query) return nodes;

    const lowerQuery = query.toLowerCase();
    return nodes.filter((node) => {
      if (node.name.toLowerCase().includes(lowerQuery)) {
        return true;
      }
      if (node.children) {
        const filteredChildren = filterTree(node.children, query);
        return filteredChildren.length > 0;
      }
      return false;
    });
  };

  const displayTree = searchQuery ? filterTree(tree, searchQuery) : tree;

  const getFileIcon = (node: TreeNode) => {
    if (node.is_directory) {
      return node.isExpanded ? FolderOpen : Folder;
    }

    const ext = node.extension?.toLowerCase();
    if (ext && ["png", "jpg", "jpeg", "gif", "svg", "webp"].includes(ext)) {
      return ImageIcon;
    }
    if (ext && ["ts", "tsx", "js", "jsx", "rs", "py", "java", "c", "cpp", "go"].includes(ext)) {
      return FileCode;
    }
    if (ext && ["json", "yaml", "yml", "toml"].includes(ext)) {
      return FileJson;
    }
    return FileText;
  };

  const renderNode = (node: TreeNode, depth: number = 0, path: string[] = []) => {
    const Icon = getFileIcon(node);
    const isSelected = node.path === selectedPath;

    return (
      <div key={node.path}>
        <button
          onClick={() => toggleDirectory(node, path)}
          className={cn(
            "flex w-full items-center gap-2 px-2 py-1.5 text-left text-sm transition-colors rounded hover:bg-surface",
            isSelected && "bg-primary/10 text-primary"
          )}
          style={{ paddingLeft: `${depth * 12 + 8}px` }}
        >
          {node.is_directory && (
            <span className="flex-shrink-0">
              {node.isLoading ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : node.isExpanded ? (
                <ChevronDown className="h-3.5 w-3.5" />
              ) : (
                <ChevronRight className="h-3.5 w-3.5" />
              )}
            </span>
          )}
          <Icon className={cn("h-4 w-4 flex-shrink-0", node.is_directory && "text-primary")} />
          <span className="truncate flex-1">{node.name}</span>
          {!node.is_directory && typeof node.size === "number" && Number.isFinite(node.size) && (
            <span className="text-xs text-text-muted flex-shrink-0">
              {formatFileSize(node.size)}
            </span>
          )}
        </button>

        {node.isExpanded && node.children && node.children.length > 0 && (
          <AnimatePresence>
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.2 }}
            >
              {node.children.map((child) => renderNode(child, depth + 1, [...path, node.name]))}
            </motion.div>
          </AnimatePresence>
        )}
      </div>
    );
  };

  if (!rootPath) {
    return (
      <div className="flex h-full items-center justify-center p-8 text-center">
        <div>
          <File className="mx-auto h-12 w-12 text-text-muted opacity-50" />
          <p className="mt-4 text-sm text-text-muted">No project selected</p>
        </div>
      </div>
    );
  }

  if (isLoading && tree.length === 0) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-8 w-8 animate-spin text-primary" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-8 text-center">
        <div>
          <p className="text-sm text-red-400">{error}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* Search */}
      <div className="flex-shrink-0 border-b border-border p-4">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
          <input
            type="text"
            placeholder="Search files..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full rounded-lg border border-border bg-surface pl-9 pr-3 py-2 text-sm text-text placeholder-text-muted focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          />
        </div>
      </div>

      {/* File tree */}
      <div className="flex-1 overflow-y-auto p-2">
        {displayTree.length === 0 ? (
          <div className="p-4 text-center text-sm text-text-muted">
            {searchQuery ? "No files match your search" : "No files found"}
          </div>
        ) : (
          displayTree.map((node) => renderNode(node))
        )}
      </div>
    </div>
  );
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Math.round((bytes / Math.pow(k, i)) * 10) / 10} ${sizes[i]}`;
}
