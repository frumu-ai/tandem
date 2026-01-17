import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  Plus,
  Trash2,
  MessageSquare,
  FolderOpen,
  Clock,
  FileText,
} from "lucide-react";
import { cn } from "@/lib/utils";

export interface Project {
  id: string;
  worktree: string;
  vcs?: string;
  time: {
    created: number;
    updated: number;
  };
}

export interface SessionSummary {
  additions: number;
  deletions: number;
  files: number;
}

export interface SessionInfo {
  id: string;
  slug?: string;
  version?: string;
  projectID: string;
  directory: string;
  title: string;
  time: {
    created: number;
    updated: number;
  };
  summary?: SessionSummary;
}

interface SessionSidebarProps {
  isOpen: boolean;
  onToggle: () => void;
  sessions: SessionInfo[];
  projects: Project[];
  currentSessionId: string | null;
  onSelectSession: (sessionId: string) => void;
  onNewChat: () => void;
  onDeleteSession: (sessionId: string) => void;
  isLoading?: boolean;
}

export function SessionSidebar({
  isOpen,
  onToggle,
  sessions,
  projects,
  currentSessionId,
  onSelectSession,
  onNewChat,
  onDeleteSession,
  isLoading,
}: SessionSidebarProps) {
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [deleteConfirm, setDeleteConfirm] = useState<string | null>(null);

  // Group sessions by project
  const sessionsByProject = sessions.reduce(
    (acc, session) => {
      const projectId = session.projectID;
      if (!acc[projectId]) {
        acc[projectId] = [];
      }
      acc[projectId].push(session);
      return acc;
    },
    {} as Record<string, SessionInfo[]>
  );

  // Sort sessions within each project by updated time (newest first)
  Object.keys(sessionsByProject).forEach((projectId) => {
    sessionsByProject[projectId].sort((a, b) => b.time.updated - a.time.updated);
  });

  // Auto-expand projects that have the current session
  useEffect(() => {
    if (currentSessionId) {
      const session = sessions.find((s) => s.id === currentSessionId);
      if (session) {
        setExpandedProjects((prev) => new Set([...prev, session.projectID]));
      }
    }
  }, [currentSessionId, sessions]);

  const toggleProject = (projectId: string) => {
    setExpandedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(projectId)) {
        next.delete(projectId);
      } else {
        next.add(projectId);
      }
      return next;
    });
  };

  const formatTime = (timestamp: number) => {
    const date = new Date(timestamp);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

    if (diffDays === 0) {
      return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    } else if (diffDays === 1) {
      return "Yesterday";
    } else if (diffDays < 7) {
      return date.toLocaleDateString([], { weekday: "short" });
    } else {
      return date.toLocaleDateString([], { month: "short", day: "numeric" });
    }
  };

  const getProjectName = (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (project) {
      // Get the last part of the path
      const parts = project.worktree.split(/[/\\]/);
      return parts[parts.length - 1] || project.worktree;
    }
    // Fallback: try to get from session directory
    const session = sessions.find((s) => s.projectID === projectId);
    if (session) {
      const parts = session.directory.split(/[/\\]/);
      return parts[parts.length - 1] || session.directory;
    }
    return "Unknown Project";
  };

  const getProjectPath = (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (project) return project.worktree;
    const session = sessions.find((s) => s.projectID === projectId);
    return session?.directory || "";
  };

  const handleDelete = (sessionId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (deleteConfirm === sessionId) {
      onDeleteSession(sessionId);
      setDeleteConfirm(null);
    } else {
      setDeleteConfirm(sessionId);
      // Auto-clear confirmation after 3 seconds
      setTimeout(() => setDeleteConfirm(null), 3000);
    }
  };

  return (
    <>
      {/* Sidebar */}
      <AnimatePresence>
        {isOpen && (
          <motion.aside
            initial={{ width: 0, opacity: 0 }}
            animate={{ width: 280, opacity: 1 }}
            exit={{ width: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="flex h-full flex-col border-r border-border bg-surface overflow-hidden"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <div className="flex items-center gap-2">
                <MessageSquare className="h-4 w-4 text-primary" />
                <span className="font-medium text-sm">Chat History</span>
              </div>
              <button
                onClick={onToggle}
                className="p-1 hover:bg-surface-elevated rounded transition-colors"
              >
                <ChevronLeft className="h-4 w-4 text-text-muted" />
              </button>
            </div>

            {/* New Chat Button */}
            <div className="p-3 border-b border-border">
              <button
                onClick={onNewChat}
                className="w-full flex items-center justify-center gap-2 px-4 py-2 bg-primary text-white rounded-lg hover:bg-primary/90 transition-colors"
              >
                <Plus className="h-4 w-4" />
                <span className="text-sm font-medium">New Chat</span>
              </button>
            </div>

            {/* Sessions List */}
            <div className="flex-1 overflow-y-auto">
              {isLoading ? (
                <div className="flex items-center justify-center py-8">
                  <div className="animate-spin h-5 w-5 border-2 border-primary border-t-transparent rounded-full" />
                </div>
              ) : Object.keys(sessionsByProject).length === 0 ? (
                <div className="flex flex-col items-center justify-center py-8 text-text-muted">
                  <MessageSquare className="h-8 w-8 mb-2 opacity-50" />
                  <p className="text-sm">No chat history</p>
                  <p className="text-xs mt-1">Start a new chat to begin</p>
                </div>
              ) : (
                <div className="py-2">
                  {Object.keys(sessionsByProject).map((projectId) => (
                    <div key={projectId} className="mb-1">
                      {/* Project Header */}
                      <button
                        onClick={() => toggleProject(projectId)}
                        className="w-full flex items-center gap-2 px-3 py-2 hover:bg-surface-elevated transition-colors"
                      >
                        <ChevronDown
                          className={cn(
                            "h-3 w-3 text-text-muted transition-transform",
                            !expandedProjects.has(projectId) && "-rotate-90"
                          )}
                        />
                        <FolderOpen className="h-4 w-4 text-amber-500" />
                        <span className="flex-1 text-sm font-medium text-text truncate text-left">
                          {getProjectName(projectId)}
                        </span>
                        <span className="text-xs text-text-subtle">
                          {sessionsByProject[projectId].length}
                        </span>
                      </button>

                      {/* Project Path */}
                      {expandedProjects.has(projectId) && (
                        <div className="px-8 pb-1">
                          <p className="text-xs text-text-subtle truncate">
                            {getProjectPath(projectId)}
                          </p>
                        </div>
                      )}

                      {/* Sessions */}
                      <AnimatePresence>
                        {expandedProjects.has(projectId) && (
                          <motion.div
                            initial={{ height: 0, opacity: 0 }}
                            animate={{ height: "auto", opacity: 1 }}
                            exit={{ height: 0, opacity: 0 }}
                            transition={{ duration: 0.15 }}
                            className="overflow-hidden"
                          >
                            {sessionsByProject[projectId].map((session) => (
                              <div
                                key={session.id}
                                onClick={() => onSelectSession(session.id)}
                                role="button"
                                tabIndex={0}
                                onKeyDown={(e) => e.key === "Enter" && onSelectSession(session.id)}
                                className={cn(
                                  "w-full flex items-start gap-2 px-3 py-2 pl-8 hover:bg-surface-elevated transition-colors group cursor-pointer",
                                  currentSessionId === session.id && "bg-primary/10"
                                )}
                              >
                                <MessageSquare
                                  className={cn(
                                    "h-4 w-4 mt-0.5 flex-shrink-0",
                                    currentSessionId === session.id
                                      ? "text-primary"
                                      : "text-text-muted"
                                  )}
                                />
                                <div className="flex-1 min-w-0 text-left">
                                  <p
                                    className={cn(
                                      "text-sm truncate",
                                      currentSessionId === session.id
                                        ? "text-primary font-medium"
                                        : "text-text"
                                    )}
                                  >
                                    {session.title || "New Chat"}
                                  </p>
                                  <div className="flex items-center gap-2 mt-0.5">
                                    <Clock className="h-3 w-3 text-text-subtle" />
                                    <span className="text-xs text-text-subtle">
                                      {formatTime(session.time.updated)}
                                    </span>
                                    {session.summary && session.summary.files > 0 && (
                                      <>
                                        <FileText className="h-3 w-3 text-text-subtle" />
                                        <span className="text-xs text-text-subtle">
                                          {session.summary.files} file
                                          {session.summary.files !== 1 ? "s" : ""}
                                        </span>
                                      </>
                                    )}
                                  </div>
                                </div>
                                {/* Delete button */}
                                <button
                                  onClick={(e) => handleDelete(session.id, e)}
                                  className={cn(
                                    "p-1 rounded transition-colors opacity-0 group-hover:opacity-100",
                                    deleteConfirm === session.id
                                      ? "bg-error/20 text-error opacity-100"
                                      : "hover:bg-surface text-text-muted hover:text-error"
                                  )}
                                  title={
                                    deleteConfirm === session.id
                                      ? "Click again to confirm"
                                      : "Delete chat"
                                  }
                                >
                                  <Trash2 className="h-3 w-3" />
                                </button>
                              </div>
                            ))}
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </motion.aside>
        )}
      </AnimatePresence>

      {/* Toggle Button (when closed) */}
      {!isOpen && (
        <button
          onClick={onToggle}
          className="absolute left-16 top-1/2 -translate-y-1/2 z-10 p-1 bg-surface-elevated border border-border rounded-r-lg hover:bg-surface transition-colors"
          title="Show chat history"
        >
          <ChevronRight className="h-4 w-4 text-text-muted" />
        </button>
      )}
    </>
  );
}
