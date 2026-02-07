import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { ChevronDown, FolderOpen, Check, Plus, Settings as SettingsIcon } from "lucide-react";
import type { UserProject } from "@/lib/tauri";

interface ProjectSwitcherProps {
  projects: UserProject[];
  activeProject: UserProject | null;
  onSwitchProject: (projectId: string) => void;
  onAddProject: () => void;
  onManageProjects: () => void;
  isLoading?: boolean;
}

export function ProjectSwitcher({
  projects,
  activeProject,
  onSwitchProject,
  onAddProject,
  onManageProjects,
  isLoading,
}: ProjectSwitcherProps) {
  const [isOpen, setIsOpen] = useState(false);

  const handleSwitchProject = (projectId: string) => {
    setIsOpen(false);
    onSwitchProject(projectId);
  };

  const handleAddProject = () => {
    setIsOpen(false);
    onAddProject();
  };

  const handleManageProjects = () => {
    setIsOpen(false);
    onManageProjects();
  };

  return (
    <div className="relative">
      {/* Dropdown Button */}
      <button
        onClick={() => setIsOpen(!isOpen)}
        disabled={isLoading}
        className="glass border-glass flex w-full items-center justify-between gap-3 rounded-lg p-3 text-left transition-all hover:bg-surface-elevated disabled:opacity-50"
      >
        <div className="flex min-w-0 flex-1 items-center gap-3">
          <FolderOpen className="h-4 w-4 flex-shrink-0 text-secondary" />
          <div className="min-w-0 flex-1">
            {activeProject ? (
              <>
                <p className="truncate text-sm font-medium text-text">{activeProject.name}</p>
                <p className="truncate text-xs text-text-subtle">{activeProject.path}</p>
              </>
            ) : (
              <p className="text-sm text-text-muted">No folder selected</p>
            )}
          </div>
        </div>
        <ChevronDown
          className={`h-4 w-4 flex-shrink-0 text-text-muted transition-transform ${
            isOpen ? "rotate-180" : ""
          }`}
        />
      </button>

      {/* Dropdown Menu */}
      <AnimatePresence>
        {isOpen && (
          <>
            {/* Backdrop */}
            <div className="fixed inset-0 z-40" onClick={() => setIsOpen(false)} />

            {/* Menu */}
            <motion.div
              initial={{ opacity: 0, y: -10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -10 }}
              transition={{ duration: 0.15 }}
              className="bg-surface-elevated border border-border absolute left-0 right-0 top-full z-50 mt-2 max-h-96 overflow-y-auto rounded-lg shadow-2xl ring-1 ring-white/5"
            >
              {/* Folders List */}
              {projects.length > 0 ? (
                <div className="p-2">
                  <p className="mb-2 px-3 text-xs font-medium text-text-subtle">Folders</p>
                  {projects.map((project) => (
                    <button
                      key={project.id}
                      onClick={() => handleSwitchProject(project.id)}
                      disabled={isLoading}
                      className="flex w-full items-center gap-3 rounded-lg p-3 text-left transition-colors hover:bg-surface-elevated disabled:opacity-50"
                    >
                      <FolderOpen className="h-4 w-4 flex-shrink-0 text-secondary" />
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium text-text">{project.name}</p>
                        <p className="truncate text-xs text-text-subtle">{project.path}</p>
                      </div>
                      {activeProject?.id === project.id && (
                        <Check className="h-4 w-4 flex-shrink-0 text-primary" />
                      )}
                    </button>
                  ))}
                </div>
              ) : (
                <div className="p-4 text-center">
                  <FolderOpen className="mx-auto mb-2 h-8 w-8 text-text-subtle" />
                  <p className="text-sm text-text-muted">No folders yet</p>
                </div>
              )}

              {/* Divider */}
              <div className="my-2 border-t border-border" />

              {/* Actions */}
              <div className="p-2">
                <button
                  onClick={handleAddProject}
                  disabled={isLoading}
                  className="flex w-full items-center gap-3 rounded-lg p-3 text-left transition-colors hover:bg-surface-elevated disabled:opacity-50"
                >
                  <Plus className="h-4 w-4 text-primary" />
                  <span className="text-sm text-primary">Add Folder</span>
                </button>
                <button
                  onClick={handleManageProjects}
                  disabled={isLoading}
                  className="flex w-full items-center gap-3 rounded-lg p-3 text-left transition-colors hover:bg-surface-elevated disabled:opacity-50"
                >
                  <SettingsIcon className="h-4 w-4 text-text-muted" />
                  <span className="text-sm text-text-muted">Manage Folders</span>
                </button>
              </div>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </div>
  );
}
