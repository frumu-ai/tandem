import { useState, useMemo } from "react";
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
  Sparkles,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { ProjectSwitcher } from "./ProjectSwitcher";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import type { UserProject } from "@/lib/tauri";
import type { RunSummary } from "@/components/orchestrate/types";

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

// Internal unified item type
interface DisplayItem {
  id: string;
  type: "chat" | "orchestrator";
  projectID: string;
  directory?: string;
  title: string;
  updatedAt: number;
  status?: string; // For orchestrator runs
  summary?: SessionSummary; // For chat sessions
}

interface SessionSidebarProps {
  isOpen: boolean;
  onToggle: () => void;
  sessions: SessionInfo[];
  runs?: RunSummary[];
  projects: Project[];
  currentSessionId: string | null;
  currentRunId?: string | null;
  onSelectSession: (sessionId: string) => void;
  onSelectRun?: (runId: string) => void;
  onNewChat: () => void;
  onOpenPacks?: () => void;
  onDeleteSession: (sessionId: string) => void;
  onDeleteRun?: (runId: string) => void;
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
  runs = [],
  projects,
  currentSessionId,
  currentRunId,
  onSelectSession,
  onSelectRun,
  onNewChat,
  onOpenPacks,
  onDeleteSession,
  onDeleteRun,
  isLoading,
  userProjects = [],
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  projectSwitcherLoading = false,
}: SessionSidebarProps) {
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [sessionToDelete, setSessionToDelete] = useState<DisplayItem | null>(null);

  // Merge and group items by project
  const itemsByProject = useMemo(() => {
    const items: DisplayItem[] = [];

    // Map chat sessions
    sessions.forEach((s) => {
      // Hide internal orchestration child/root sessions from the chat list.
      // Orchestration runs have their own sidebar items, and task/resume sessions
      // should not clutter the user's chat history.
      if (
        s.title?.startsWith("Orchestrator Task ") ||
        s.title?.startsWith("Orchestrator Resume:")
      ) {
        return;
      }
      items.push({
        id: s.id,
        type: "chat",
        projectID: s.projectID,
        directory: s.directory,
        title: s.title,
        updatedAt: s.time.updated,
        summary: s.summary,
      });
    });

    // Map orchestrator runs
    // Note: Orchestrator runs listed from disk usually belong to the active workspace.
    // If we have an activeProject, we assign them to it.
    // If not, we might need a fallback or they won't show up correctly in project groups.
    if (activeProject) {
      // Find a matching project ID from existing sessions if possible
      // to ensure they group together under the same header.
      const normalizedActivePath = activeProject.path.toLowerCase().replace(/\\/g, "/");

      const matchingSession = sessions.find(
        (s) => s.directory && s.directory.toLowerCase().replace(/\\/g, "/") === normalizedActivePath
      );

      const targetProjectID = matchingSession ? matchingSession.projectID : activeProject.id;

      // Create a Set of session IDs used by runs to filter them out of the chat list
      const runSessionIds = new Set(runs.map((r) => r.session_id));

      // Remove chat sessions that correspond to orchestrator runs
      const filteredItems = items.filter(
        (item) => item.type !== "chat" || !runSessionIds.has(item.id)
      );

      // Clear items and add back filtered ones
      items.length = 0;
      items.push(...filteredItems);

      runs.forEach((r) => {
        items.push({
          id: r.run_id,
          type: "orchestrator",
          projectID: targetProjectID, // Use matched ID to ensure grouping
          directory: activeProject.path,
          title: r.objective,
          updatedAt: new Date(r.updated_at).getTime(),
          status: r.status,
        });
      });
    }

    // Group by project
    const grouped = items.reduce(
      (acc, item) => {
        if (!acc[item.projectID]) {
          acc[item.projectID] = [];
        }
        acc[item.projectID].push(item);
        return acc;
      },
      {} as Record<string, DisplayItem[]>
    );

    // Sort items within each project by updated time (newest first)
    Object.keys(grouped).forEach((projectId) => {
      grouped[projectId].sort((a, b) => b.updatedAt - a.updatedAt);
    });

    return grouped;
  }, [sessions, runs, activeProject]);

  const autoExpandedProjectIds = useMemo(() => {
    const ids = new Set<string>();

    if (currentSessionId) {
      const session = sessions.find((s) => s.id === currentSessionId);
      if (session) {
        ids.add(session.projectID);
      }
    }

    if (currentRunId && activeProject) {
      const run = runs.find((r) => r.run_id === currentRunId);
      if (run) {
        ids.add(activeProject.id);
      }
    }

    return ids;
  }, [currentSessionId, currentRunId, sessions, runs, activeProject]);

  const isProjectExpanded = (projectId: string) =>
    expandedProjects.has(projectId) || autoExpandedProjectIds.has(projectId);

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
    // First, try to match with our userProjects
    const matchingUserProject = userProjects.find((up) => up.id === projectId);
    if (matchingUserProject) return matchingUserProject.name;

    // Try mapping via directory from known sessions
    const sampleItem = itemsByProject[projectId]?.[0];
    if (sampleItem && sampleItem.directory) {
      const normalizedItemDir = sampleItem.directory.toLowerCase().replace(/\\/g, "/");
      const userProj = userProjects.find((up) => {
        const normalizedProjectPath = up.path.toLowerCase().replace(/\\/g, "/");
        return (
          normalizedItemDir.includes(normalizedProjectPath) ||
          normalizedProjectPath.includes(normalizedItemDir)
        );
      });
      if (userProj) return userProj.name;
    }

    // Try from OpenCode projects
    const project = projects.find((p) => p.id === projectId);
    if (project && project.worktree && project.worktree !== "/") {
      const parts = project.worktree.split(/[/\\]/).filter((p) => p.length > 0);
      if (parts.length > 0) return parts[parts.length - 1];
    }

    // Fallback: try to get from item directory
    if (sampleItem?.directory) {
      const parts = sampleItem.directory.split(/[/\\]/).filter((p) => p.length > 0);
      if (parts.length > 0) return parts[parts.length - 1];
    }

    // Fallback for current active project if we're rendering its items
    if (activeProject && activeProject.id === projectId) {
      return activeProject.name;
    }

    return "Unknown Folder";
  };

  const getProjectPath = (projectId: string) => {
    const matchingUserProject = userProjects.find((up) => up.id === projectId);
    if (matchingUserProject) return matchingUserProject.path;

    // Logic similar to getProjectName but returning path
    const sampleItem = itemsByProject[projectId]?.[0];
    if (sampleItem && sampleItem.directory) {
      return sampleItem.directory;
    }

    const project = projects.find((p) => p.id === projectId);
    if (project) return project.worktree;

    return "";
  };

  const handleDelete = (item: DisplayItem, e: React.MouseEvent) => {
    e.stopPropagation();
    setSessionToDelete(item);
  };

  const confirmDelete = () => {
    if (!sessionToDelete) return;
    if (sessionToDelete.type === "chat") {
      onDeleteSession(sessionToDelete.id);
      setSessionToDelete(null);
      return;
    }
    if (sessionToDelete.type === "orchestrator") {
      onDeleteRun?.(sessionToDelete.id);
      setSessionToDelete(null);
    }
  };

  const cancelDelete = () => {
    setSessionToDelete(null);
  };

  const handleItemSelect = (item: DisplayItem) => {
    if (item.type === "chat") {
      onSelectSession(item.id);
    } else if (item.type === "orchestrator" && onSelectRun) {
      onSelectRun(item.id);
    }
  };

  return (
    <>
      {isOpen && (
        <div className="flex h-full w-full flex-col overflow-hidden">
          {/* Header */}
          <div className="flex items-center justify-between border-b border-border px-4 py-3">
            <div className="flex items-center gap-2">
              <FolderOpen className="h-4 w-4 text-primary" />
              <span className="text-sm font-medium">Sessions</span>
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

            {onOpenPacks && (
              <button
                onClick={onOpenPacks}
                className="mt-3 flex w-full items-center justify-center gap-2 rounded-lg border border-border bg-surface-elevated px-4 py-2 text-text transition-all hover:bg-surface"
              >
                <Sparkles className="h-4 w-4 text-accent" />
                <span className="text-sm font-medium">Starter Packs</span>
              </button>
            )}
          </div>

          {/* Sessions List */}
          <div className="flex-1 overflow-y-auto">
            {isLoading ? (
              <div className="flex items-center justify-center py-8">
                <div className="h-5 w-5 animate-spin rounded-full border-2 border-primary border-t-transparent" />
              </div>
            ) : Object.keys(itemsByProject).length === 0 ? (
              <div className="flex flex-col items-center justify-center py-8 text-text-muted">
                <MessageSquare className="mb-2 h-8 w-8 opacity-50" />
                <p className="text-sm">No history</p>
                <p className="mt-1 text-xs text-text-subtle">Start a new chat to begin</p>
              </div>
            ) : (
              <div className="py-2">
                {Object.keys(itemsByProject).map((projectId) => (
                  <div key={projectId} className="mb-1">
                    {/* Project Header */}
                    <button
                      onClick={() => toggleProject(projectId)}
                      className="flex w-full items-center gap-2 px-3 py-2 transition-colors hover:bg-surface-elevated"
                    >
                      <ChevronDown
                        className={cn(
                          "h-3 w-3 text-text-muted transition-transform",
                          !isProjectExpanded(projectId) && "-rotate-90"
                        )}
                      />
                      <FolderOpen className="h-4 w-4 text-warning" />
                      <span className="flex-1 truncate text-left text-sm font-medium text-text">
                        {getProjectName(projectId)}
                      </span>
                      <span className="text-xs text-text-subtle">
                        {itemsByProject[projectId].length}
                      </span>
                    </button>

                    {/* Project Path */}
                    {isProjectExpanded(projectId) && (
                      <div className="px-8 pb-1">
                        <p className="truncate text-xs text-text-subtle">
                          {getProjectPath(projectId)}
                        </p>
                      </div>
                    )}

                    {/* Items */}
                    <AnimatePresence>
                      {isProjectExpanded(projectId) && (
                        <motion.div
                          initial={{ height: 0, opacity: 0 }}
                          animate={{ height: "auto", opacity: 1 }}
                          exit={{ height: 0, opacity: 0 }}
                          transition={{ duration: 0.15 }}
                          className="overflow-hidden"
                        >
                          {itemsByProject[projectId].map((item) => (
                            <div
                              key={item.id}
                              onClick={() => handleItemSelect(item)}
                              role="button"
                              tabIndex={0}
                              onKeyDown={(e) => e.key === "Enter" && handleItemSelect(item)}
                              className={cn(
                                "group relative flex w-full cursor-pointer items-start gap-2 px-3 py-2 pl-10 transition-colors hover:bg-surface-elevated",
                                "before:absolute before:left-4 before:top-1/2 before:h-5 before:w-1 before:-translate-y-1/2 before:rounded-full before:bg-primary/40",
                                (currentSessionId === item.id || currentRunId === item.id) &&
                                  "bg-primary/10 before:bg-primary"
                              )}
                            >
                              {item.type === "orchestrator" ? (
                                <Sparkles
                                  className={cn(
                                    "mt-0.5 h-4 w-4 flex-shrink-0",
                                    currentRunId === item.id ? "text-primary" : "text-purple-400"
                                  )}
                                />
                              ) : (
                                <MessageSquare
                                  className={cn(
                                    "mt-0.5 h-4 w-4 flex-shrink-0",
                                    currentSessionId === item.id
                                      ? "text-primary"
                                      : "text-text-muted"
                                  )}
                                />
                              )}

                              <div className="min-w-0 flex-1 text-left">
                                <p
                                  className={cn(
                                    "truncate text-sm",
                                    currentSessionId === item.id || currentRunId === item.id
                                      ? "font-medium text-primary"
                                      : "text-text"
                                  )}
                                >
                                  {item.title ||
                                    (item.type === "chat" ? "New Chat" : "Untitled Run")}
                                </p>
                                <div className="mt-0.5 flex items-center gap-2">
                                  {item.status ? (
                                    <span
                                      className={cn(
                                        "text-[10px] uppercase font-medium",
                                        item.status === "completed"
                                          ? "text-emerald-500"
                                          : item.status === "failed"
                                            ? "text-red-500"
                                            : "text-text-muted"
                                      )}
                                    >
                                      {item.status.replace("_", " ")}
                                    </span>
                                  ) : (
                                    <>
                                      <Clock className="h-3 w-3 text-text-subtle" />
                                      <span className="text-xs text-text-subtle">
                                        {formatTime(item.updatedAt)}
                                      </span>
                                    </>
                                  )}

                                  {item.summary && item.summary.files > 0 && (
                                    <>
                                      <FileText className="h-3 w-3 text-text-subtle" />
                                      <span className="text-xs text-text-subtle">
                                        {item.summary.files} file
                                        {item.summary.files !== 1 ? "s" : ""}
                                      </span>
                                    </>
                                  )}
                                </div>
                              </div>
                              {/* Delete button */}
                              <button
                                onClick={(e) => handleDelete(item, e)}
                                className={cn(
                                  "rounded p-1 text-text-muted opacity-0 transition-colors hover:bg-surface hover:text-error group-hover:opacity-100",
                                  item.type === "orchestrator" && !onDeleteRun ? "hidden" : ""
                                )}
                                title={item.type === "chat" ? "Delete chat" : "Delete run"}
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
        title={sessionToDelete?.type === "orchestrator" ? "Delete Run" : "Delete Chat"}
        message={`Are you sure you want to delete "${sessionToDelete?.title || (sessionToDelete?.type === "orchestrator" ? "this run" : "this chat")}"? This action cannot be undone.`}
        confirmText="Delete"
        cancelText="Cancel"
        variant="danger"
        onConfirm={confirmDelete}
        onCancel={cancelDelete}
      />
    </>
  );
}
