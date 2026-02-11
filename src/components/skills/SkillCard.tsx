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
          <span className="font-medium text-text">{skill.name}</span>
        </div>
        <p className="text-sm text-text-muted">{skill.description}</p>
        {skill.parse_error && (
          <p className="mt-1 text-xs text-error">Invalid skill metadata: {skill.parse_error}</p>
        )}
        <div className="mt-2 flex flex-wrap gap-1">
          {skill.version && (
            <span className="rounded border border-border px-1.5 py-0.5 text-[10px] text-text-subtle">
              v{skill.version}
            </span>
          )}
          {skill.author && (
            <span className="rounded border border-border px-1.5 py-0.5 text-[10px] text-text-subtle">
              {skill.author}
            </span>
          )}
          {skill.requires?.slice(0, 3).map((req) => (
            <span
              key={req}
              className="rounded border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary"
            >
              {req}
            </span>
          ))}
          {skill.tags?.slice(0, 2).map((tag) => (
            <span
              key={tag}
              className="rounded border border-border px-1.5 py-0.5 text-[10px] text-text-subtle"
            >
              {tag}
            </span>
          ))}
        </div>
        <p className="text-xs text-text-subtle font-mono">
          {skill.location === "project" ? "Folder" : "Global"} - {skill.path}
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
