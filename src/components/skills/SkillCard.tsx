import { useState } from "react";
import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { deleteSkill, type SkillInfo } from "@/lib/tauri";

interface SkillCardProps {
  skill: SkillInfo;
  onDelete: () => void;
}

export function SkillCard({ skill, onDelete }: SkillCardProps) {
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const handleDelete = async () => {
    try {
      setDeleting(true);
      await deleteSkill(skill.name, skill.location);
      onDelete();
    } catch (error) {
      console.error("Failed to delete skill:", error);
    } finally {
      setDeleting(false);
      setConfirmDelete(false);
    }
  };

  return (
    <div className="flex items-center justify-between rounded-lg border border-border bg-surface-elevated p-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-lg">{skill.location === "project" ? "📦" : "🌍"}</span>
          <span className="font-medium text-text">{skill.name}</span>
        </div>
        <p className="text-sm text-text-muted">{skill.description}</p>
        <p className="text-xs text-text-subtle font-mono">
          📍 {skill.location === "project" ? "Project" : "Global"} • {skill.path}
        </p>
      </div>
      <div className="flex items-center gap-2">
        {confirmDelete ? (
          <>
            <Button
              size="sm"
              variant="ghost"
              onClick={handleDelete}
              disabled={deleting}
              className="text-error hover:bg-error/10"
            >
              {deleting ? "Deleting..." : "Confirm"}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setConfirmDelete(false)}
              disabled={deleting}
            >
              Cancel
            </Button>
          </>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => setConfirmDelete(true)}
            className="text-text-subtle hover:text-error"
            title="Delete skill"
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        )}
      </div>
    </div>
  );
}
