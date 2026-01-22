import { useState, useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { SkillCard } from "./SkillCard";
import { importSkill, type SkillInfo, type SkillLocation } from "@/lib/tauri";
import { openUrl } from "@tauri-apps/plugin-opener";

interface SkillsPanelProps {
  skills: SkillInfo[];
  onRefresh: () => void;
  projectPath?: string;
  onRestartSidecar?: () => Promise<void>;
}

export function SkillsPanel({
  skills,
  onRefresh,
  projectPath,
  onRestartSidecar,
}: SkillsPanelProps) {
  const [content, setContent] = useState("");
  // Default to global if no project path available
  const [location, setLocation] = useState<SkillLocation>(projectPath ? "project" : "global");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    if (!content.trim()) {
      setError("Please paste SKILL.md content");
      return;
    }

    try {
      setSaving(true);
      setError(null);
      await importSkill(content, location);
      setContent("");
      await onRefresh();

      // Trigger sidecar restart via callback
      if (onRestartSidecar) {
        await onRestartSidecar();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to import skill");
    } finally {
      setSaving(false);
    }
  };

  const handleCreateBlank = () => {
    setContent(`---
name: my-skill
description: What this skill does
---

Instructions for the AI...
`);
  };

  // Extract project name from path for display
  const projectName = projectPath ? projectPath.split(/[\\/]/).pop() || "Active Project" : null;
  const hasActiveProject = !!projectPath;

  // Auto-switch to global if project becomes unavailable
  useEffect(() => {
    if (!hasActiveProject && location === "project") {
      setLocation("global");
    }
  }, [hasActiveProject, location]);

  const projectSkills = skills.filter((s) => s.location === "project");
  const globalSkills = skills.filter((s) => s.location === "global");

  return (
    <div className="space-y-6">
      {/* Add a skill section */}
      <div className="space-y-3">
        <label className="text-sm font-medium text-text">Add a skill</label>
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          placeholder="Paste SKILL.md content here..."
          rows={10}
          className="w-full rounded-lg border border-border bg-surface p-3 font-mono text-sm text-text placeholder:text-text-subtle focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
        />

        {error && (
          <div className="rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
            {error}
          </div>
        )}

        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <span className="text-sm text-text-muted">📍 Save to:</span>
            <label className="flex items-center gap-2">
              <input
                type="radio"
                name="location"
                value="project"
                checked={location === "project"}
                onChange={(e) => setLocation(e.target.value as SkillLocation)}
                disabled={!hasActiveProject}
                className="h-4 w-4 border-border text-primary focus:ring-primary disabled:cursor-not-allowed disabled:opacity-50"
              />
              <span className={`text-sm ${hasActiveProject ? "text-text" : "text-text-muted"}`}>
                {hasActiveProject ? (
                  <>
                    Active Project:{" "}
                    <span className="font-bold" style={{ color: "var(--color-primary)" }}>
                      {projectName}
                    </span>
                    <span className="ml-2 text-text-subtle text-xs">(.opencode/skill/)</span>
                  </>
                ) : (
                  "Project (no project selected)"
                )}
              </span>
            </label>
            <label className="flex items-center gap-2">
              <input
                type="radio"
                name="location"
                value="global"
                checked={location === "global"}
                onChange={(e) => setLocation(e.target.value as SkillLocation)}
                className="h-4 w-4 border-border text-primary focus:ring-primary"
              />
              <span className="text-sm text-text">Global (~/.config/opencode/skills/)</span>
            </label>
          </div>

          <div className="flex items-center gap-2">
            <Button variant="ghost" onClick={handleCreateBlank} disabled={saving}>
              Create Blank
            </Button>
            <Button onClick={handleSave} disabled={!content.trim() || saving}>
              {saving ? "Saving..." : "Save"}
            </Button>
          </div>
        </div>
      </div>

      {/* Installed skills */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-medium text-text">Installed skills ({skills.length})</h3>
        </div>

        {skills.length === 0 ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-6 text-center">
            <p className="text-sm text-text-muted">No skills detected in `.opencode / skill / `.</p>
          </div>
        ) : (
          <div className="space-y-3">
            {projectSkills.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs font-medium text-text-subtle">📦 Project Skills</p>
                {projectSkills.map((skill) => (
                  <SkillCard key={skill.path} skill={skill} onDelete={onRefresh} />
                ))}
              </div>
            )}

            {globalSkills.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs font-medium text-text-subtle">🌍 Global Skills</p>
                {globalSkills.map((skill) => (
                  <SkillCard key={skill.path} skill={skill} onDelete={onRefresh} />
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Help links */}
      <div className="space-y-2 rounded-lg border border-border bg-surface-elevated/50 p-4 text-sm">
        <p className="text-text-muted">
          💡 The AI automatically uses installed skills when relevant - no selection needed.
        </p>
        <div className="text-text-muted">
          <p className="font-medium">📚 Find skills to copy:</p>
          <ul className="ml-4 mt-1 list-disc space-y-1 text-xs">
            <li>
              <button
                onClick={() => openUrl("https://github.com/VoltAgent/awesome-claude-skills")}
                className="text-primary hover:underline cursor-pointer"
              >
                Awesome Claude Skills
              </button>{" "}
              - Curated list (official + community)
            </li>
            <li>
              <button
                onClick={() => openUrl("https://skillhub.club")}
                className="text-primary hover:underline cursor-pointer"
              >
                SkillHub
              </button>{" "}
              - 7,000+ community skills
            </li>
            <li>
              <button
                onClick={() => openUrl("https://github.com/search?q=SKILL.md&type=code")}
                className="text-primary hover:underline cursor-pointer"
              >
                GitHub
              </button>{" "}
              - Search "SKILL.md"
            </li>
            <li>
              <button
                onClick={() => openUrl("https://code.claude.com/docs/en/skills")}
                className="text-primary hover:underline cursor-pointer"
              >
                Claude Code Docs
              </button>{" "}
              - Official documentation
            </li>
          </ul>
        </div>
      </div>
    </div>
  );
}
