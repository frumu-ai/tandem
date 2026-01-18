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
import { ProjectSwitcher } from "./ProjectSwitcher";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import type { UserProject } from "@/lib/tauri";

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
  // Project switcher props
  userProjects?: UserProject[];
  activeProject?: UserProject | null;
  onSwitchProject?: (projectId: string) => void;
  onAddProject?: () => void;
  onManageProjects?: () => void;
  projectSwitcherLoading?: boolean;
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
  userProjects = [],
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  projectSwitcherLoading = false,
}: SessionSidebarProps) {
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [sessionToDelete, setSessionToDelete] = useState<SessionInfo | null>(null);

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
        // eslint-disable-next-line react-hooks/set-state-in-effect
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
    // First, try to match with our userProjects by checking if the session directory matches
    const session = sessions.find((s) => s.projectID === projectId);
    if (session?.directory) {
      // Normalize paths for comparison
      const normalizedSessionDir = session.directory.toLowerCase().replace(/\\/g, "/");

      // Find a userProject that matches this session's directory
      const matchingUserProject = userProjects.find((up) => {
        const normalizedProjectPath = up.path.toLowerCase().replace(/\\/g, "/");
        return (
          normalizedSessionDir.includes(normalizedProjectPath) ||
          normalizedProjectPath.includes(normalizedSessionDir)
        );
      });
      if (matchingUserProject) {
        return matchingUserProject.name;
      }
    }

    // Try from OpenCode projects
    const project = projects.find((p) => p.id === projectId);
    if (project && project.worktree && project.worktree !== "/") {
      // Get the last non-empty part of the path
      const parts = project.worktree.split(/[/\\]/).filter((p) => p.length > 0);
      if (parts.length > 0) {
        return parts[parts.length - 1];
      }
    }

    // Fallback: try to get from session directory
    if (session?.directory) {
      const parts = session.directory.split(/[/\\]/).filter((p) => p.length > 0);
      if (parts.length > 0) {
        return parts[parts.length - 1];
      }
    }

    return "Unknown Project";
  };

  const getProjectPath = (projectId: string) => {
    // First check if we have a matching userProject
    const session = sessions.find((s) => s.projectID === projectId);
    if (session?.directory) {
      const normalizedSessionDir = session.directory.toLowerCase().replace(/\\/g, "/");

      const matchingUserProject = userProjects.find((up) => {
        const normalizedProjectPath = up.path.toLowerCase().replace(/\\/g, "/");
        return (
          normalizedSessionDir.includes(normalizedProjectPath) ||
          normalizedProjectPath.includes(normalizedSessionDir)
        );
      });
      if (matchingUserProject) {
        return matchingUserProject.path;
      }
    }

    const project = projects.find((p) => p.id === projectId);
    if (project) return project.worktree;
    return session?.directory || "";
  };

  const handleDelete = (session: SessionInfo, e: React.MouseEvent) => {
    e.stopPropagation();
    setSessionToDelete(session);
  };

  const confirmDelete = () => {
    if (sessionToDelete) {
      onDeleteSession(sessionToDelete.id);
      setSessionToDelete(null);
    }
  };

  const cancelDelete = () => {
    setSessionToDelete(null);
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
            className="glass flex h-full flex-col border-r border-white/10 overflow-hidden"
          >
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-white/10">
              <div className="flex items-center gap-2">
                <MessageSquare className="h-4 w-4 text-primary" />
                <span className="font-medium text-sm">Chat History</span>
              </div>
              <button
                onClick={onToggle}
                className="p-1 hover:bg-surface-elevated/70 rounded transition-colors"
              >
                <ChevronLeft className="h-4 w-4 text-text-muted" />
              </button>
            </div>

            {/* Project Switcher */}
            {onSwitchProject && onAddProject && onManageProjects && (
              <div className="p-3 border-b border-white/10">
                <ProjectSwitcher
                  projects={userProjects}
                  activeProject={activeProject || null}
                  onSwitchProject={onSwitchProject}
                  onAddProject={onAddProject}
                  onManageProjects={onManageProjects}
                  isLoading={projectSwitcherLoading}
                />
              </div>
            )}

            {/* New Chat Button */}
            <div className="p-3 border-b border-white/10">
              <button
                onClick={onNewChat}
                className="w-full flex items-center justify-center gap-2 px-4 py-2 rounded-lg text-white transition-all bg-gradient-to-r from-primary to-secondary hover:shadow-[0_0_12px_rgba(59,130,246,0.45)]"
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
                        className="w-full flex items-center gap-2 px-3 py-2 hover:bg-surface-elevated/70 transition-colors"
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
                                  "relative w-full flex items-start gap-2 px-3 py-2 pl-10 hover:bg-surface-elevated/70 transition-colors group cursor-pointer",
                                  "before:absolute before:left-4 before:top-1/2 before:-translate-y-1/2 before:h-5 before:w-1 before:rounded-full before:bg-primary/40",
                                  currentSessionId === session.id &&
                                    "bg-primary/10 before:bg-primary before:shadow-[0_0_8px_rgba(59,130,246,0.6)]"
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
                                  onClick={(e) => handleDelete(session, e)}
                                  className="p-1 rounded transition-colors opacity-0 group-hover:opacity-100 hover:bg-surface text-text-muted hover:text-error"
                                  title="Delete chat"
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
          className="absolute left-16 top-1/2 -translate-y-1/2 z-10 p-1 bg-surface-elevated border-glass rounded-r-lg hover:bg-surface transition-colors"
          title="Show chat history"
        >
          <ChevronRight className="h-4 w-4 text-text-muted" />
        </button>
      )}

      {/* Delete Confirmation Dialog */}
      <ConfirmDialog
        isOpen={sessionToDelete !== null}
        title="Delete Chat"
        message={`Are you sure you want to delete "${sessionToDelete?.title || "this chat"}"? This action cannot be undone.`}
        confirmText="Delete"
        cancelText="Cancel"
        variant="danger"
        onConfirm={confirmDelete}
        onCancel={cancelDelete}
      />
    </>
  );
}
