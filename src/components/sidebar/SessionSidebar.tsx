import { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  ChevronLeft,
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
      {isOpen && (
        <div className="flex h-full w-full flex-col overflow-hidden">
          {/* Header */}
          <div className="flex items-center justify-between border-b border-border px-4 py-3">
            <div className="flex items-center gap-2">
              <MessageSquare className="h-4 w-4 text-primary" />
              <span className="text-sm font-medium">Chat History</span>
            </div>
            <button
              onClick={onToggle}
              className="rounded p-1 transition-colors hover:bg-surface-elevated"
              title="Hide sidebar"
            >
              <ChevronLeft className="h-4 w-4 text-text-muted" />
            </button>
          </div>

          {/* Project Switcher */}
          {onSwitchProject && onAddProject && onManageProjects && (
            <div className="border-b border-border p-4">
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
          <div className="border-b border-border p-4">
            <button
              onClick={onNewChat}
              className="flex w-full items-center justify-center gap-2 rounded-lg bg-gradient-to-r from-primary to-secondary px-4 py-2 text-white transition-all hover:shadow-lg hover:shadow-black/30"
            >
              <Plus className="h-4 w-4" />
              <span className="text-sm font-medium">New Chat</span>
            </button>
          </div>

          {/* Sessions List */}
          <div className="flex-1 overflow-y-auto">
            {isLoading ? (
              <div className="flex items-center justify-center py-8">
                <div className="h-5 w-5 animate-spin rounded-full border-2 border-primary border-t-transparent" />
              </div>
            ) : Object.keys(sessionsByProject).length === 0 ? (
              <div className="flex flex-col items-center justify-center py-8 text-text-muted">
                <MessageSquare className="mb-2 h-8 w-8 opacity-50" />
                <p className="text-sm">No chat history</p>
                <p className="mt-1 text-xs text-text-subtle">Start a new chat to begin</p>
              </div>
            ) : (
              <div className="py-2">
                {Object.keys(sessionsByProject).map((projectId) => (
                  <div key={projectId} className="mb-1">
                    {/* Project Header */}
                    <button
                      onClick={() => toggleProject(projectId)}
                      className="flex w-full items-center gap-2 px-3 py-2 transition-colors hover:bg-surface-elevated"
                    >
                      <ChevronDown
                        className={cn(
                          "h-3 w-3 text-text-muted transition-transform",
                          !expandedProjects.has(projectId) && "-rotate-90"
                        )}
                      />
                      <FolderOpen className="h-4 w-4 text-warning" />
                      <span className="flex-1 truncate text-left text-sm font-medium text-text">
                        {getProjectName(projectId)}
                      </span>
                      <span className="text-xs text-text-subtle">
                        {sessionsByProject[projectId].length}
                      </span>
                    </button>

                    {/* Project Path */}
                    {expandedProjects.has(projectId) && (
                      <div className="px-8 pb-1">
                        <p className="truncate text-xs text-text-subtle">
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
                                "group relative flex w-full cursor-pointer items-start gap-2 px-3 py-2 pl-10 transition-colors hover:bg-surface-elevated",
                                "before:absolute before:left-4 before:top-1/2 before:h-5 before:w-1 before:-translate-y-1/2 before:rounded-full before:bg-primary/40",
                                currentSessionId === session.id && "bg-primary/10 before:bg-primary"
                              )}
                            >
                              <MessageSquare
                                className={cn(
                                  "mt-0.5 h-4 w-4 flex-shrink-0",
                                  currentSessionId === session.id
                                    ? "text-primary"
                                    : "text-text-muted"
                                )}
                              />
                              <div className="min-w-0 flex-1 text-left">
                                <p
                                  className={cn(
                                    "truncate text-sm",
                                    currentSessionId === session.id
                                      ? "font-medium text-primary"
                                      : "text-text"
                                  )}
                                >
                                  {session.title || "New Chat"}
                                </p>
                                <div className="mt-0.5 flex items-center gap-2">
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
                                className="rounded p-1 text-text-muted opacity-0 transition-colors hover:bg-surface hover:text-error group-hover:opacity-100"
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
        </div>
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
