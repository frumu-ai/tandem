import { useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { SkillCard } from "./SkillCard";
import {
  importSkill,
  installSkillTemplate,
  listSkillTemplates,
  type SkillInfo,
  type SkillLocation,
  type SkillTemplateInfo,
} from "@/lib/tauri";
import { openUrl } from "@tauri-apps/plugin-opener";
import { PythonSetupWizard } from "@/components/python";

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
  const runtimePillClass = (runtime: string) => {
    switch (runtime.toLowerCase()) {
      case "python":
        return "border-yellow-500/20 bg-yellow-500/10 text-yellow-500";
      case "node":
        return "border-emerald-500/20 bg-emerald-500/10 text-emerald-200";
      case "bash":
        return "border-sky-500/20 bg-sky-500/10 text-sky-200";
      default:
        return "border-border bg-surface text-text-subtle";
    }
  };

  const [query, setQuery] = useState("");
  const [content, setContent] = useState("");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const installedRef = useRef<HTMLDivElement | null>(null);

  // Default to global if no project path available
  const [location, setLocation] = useState<SkillLocation>(projectPath ? "project" : "global");

  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [templates, setTemplates] = useState<SkillTemplateInfo[]>([]);
  const [templatesLoading, setTemplatesLoading] = useState(false);
  const [installingTemplateId, setInstallingTemplateId] = useState<string | null>(null);
  const [showPythonWizard, setShowPythonWizard] = useState(false);

  // Extract project name from path for display
  const projectName = projectPath ? projectPath.split(/[\\/]/).pop() || "Active Folder" : null;
  const hasActiveProject = !!projectPath;

  useEffect(() => {
    if (!hasActiveProject && location === "project") {
      setLocation("global");
    }
  }, [hasActiveProject, location]);

  useEffect(() => {
    (async () => {
      try {
        setTemplatesLoading(true);
        setTemplates(await listSkillTemplates());
      } catch (e) {
        // Non-fatal: templates are a convenience feature.
        console.warn("Failed to load skill templates:", e);
      } finally {
        setTemplatesLoading(false);
      }
    })();
  }, []);

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
    setAdvancedOpen(true);
    setContent(`---
name: my-skill
description: What this skill does
---

Instructions for the AI...
`);
  };

  const handleInstallTemplate = async (templateId: string) => {
    try {
      setInstallingTemplateId(templateId);
      setError(null);

      await installSkillTemplate(templateId, location);
      await onRefresh();

      if (onRestartSidecar) await onRestartSidecar();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to install starter skill");
    } finally {
      setInstallingTemplateId(null);
    }
  };

  const queryLower = query.trim().toLowerCase();

  const filteredTemplates = useMemo(() => {
    if (!queryLower) return templates;
    return templates.filter((t) => {
      const hay = `${t.name} ${t.description}`.toLowerCase();
      return hay.includes(queryLower);
    });
  }, [templates, queryLower]);

  const filteredSkills = useMemo(() => {
    if (!queryLower) return skills;
    return skills.filter((s) => {
      const hay = `${s.name} ${s.description}`.toLowerCase();
      return hay.includes(queryLower);
    });
  }, [skills, queryLower]);

  const allProjectSkills = skills.filter((s) => s.location === "project");
  const allGlobalSkills = skills.filter((s) => s.location === "global");

  const projectSkills = filteredSkills.filter((s) => s.location === "project");
  const globalSkills = filteredSkills.filter((s) => s.location === "global");

  return (
    <div className="space-y-6">
      {error && (
        <div className="rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
          {error}
        </div>
      )}

      {/* Location choice */}
      <div className="flex flex-wrap items-center gap-4 rounded-lg border border-border bg-surface-elevated/50 p-3">
        <span className="text-sm text-text-muted">Save to:</span>
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
                Active Folder:{" "}
                <span className="font-bold" style={{ color: "var(--color-primary)" }}>
                  {projectName}
                </span>
                <span className="ml-2 text-text-subtle text-xs">(.opencode/skill/)</span>
              </>
            ) : (
              "Folder (no folder selected)"
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

      {/* Runtime note */}
      <div className="rounded-lg border border-border bg-surface-elevated/50 p-3 text-xs text-text-muted">
        <div className="flex items-center justify-between gap-3">
          <p className="font-medium text-text">Runtime note</p>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setShowPythonWizard(true)}
            className="h-7 px-2 text-[11px]"
          >
            Set up Python (venv)
          </Button>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <span className="text-[11px] text-text-subtle">May require:</span>
          <span className="rounded-full border border-yellow-500/20 bg-yellow-500/10 px-2 py-0.5 text-xs text-yellow-500">
            Python
          </span>
          <span className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-0.5 text-xs text-emerald-200">
            Node
          </span>
          <span className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2 py-0.5 text-xs text-sky-200">
            Bash
          </span>
        </div>
        <p className="mt-1">
          Some skills and packs may ask Tandem to run local tools (Python, Node, bash, etc.). Tandem
          does not bundle these runtimes. Installing a skill does not run anything by itself; a
          runtime is only needed if a skill instructs the agent to execute commands.
        </p>
      </div>
      {showPythonWizard && <PythonSetupWizard onClose={() => setShowPythonWizard(false)} />}

      {/* Search */}
      <div className="max-w-md">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search skills (youtube, writing, data...)"
        />
      </div>

      {/* Installed skills summary */}
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-surface-elevated/50 p-3">
        <div className="min-w-0">
          <p className="text-sm font-medium text-text">Installed skills</p>
          <p className="mt-0.5 text-xs text-text-muted">
            Folder: {allProjectSkills.length} • Global: {allGlobalSkills.length} • Delete with the
            trash icon.
          </p>
        </div>
        <Button
          size="sm"
          variant="secondary"
          onClick={() =>
            installedRef.current?.scrollIntoView({ behavior: "smooth", block: "start" })
          }
          disabled={skills.length === 0}
          className="h-8"
        >
          Jump to installed
        </Button>
      </div>

      {/* Starter templates */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <label className="text-sm font-medium text-text">
            Starter skills
            {queryLower ? ` (${filteredTemplates.length} of ${templates.length})` : ""}
          </label>
          <span className="text-xs text-text-subtle">Quick adds (offline)</span>
        </div>

        {templatesLoading ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
            Loading starter skills...
          </div>
        ) : filteredTemplates.length === 0 ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
            {templates.length === 0
              ? "No starter skills found."
              : "No starter skills match your search."}
          </div>
        ) : (
          <div className="grid gap-3 md:grid-cols-2">
            {filteredTemplates.map((t) => (
              <div
                key={t.id}
                className="relative rounded-lg border border-border bg-surface-elevated p-4 pb-10"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-sm font-semibold text-text">{t.name}</p>
                    <p className="mt-1 text-xs text-text-muted">{t.description}</p>
                  </div>
                  <Button
                    size="sm"
                    onClick={() => handleInstallTemplate(t.id)}
                    disabled={!!installingTemplateId}
                  >
                    {installingTemplateId === t.id ? "Installing..." : "Install"}
                  </Button>
                </div>

                {t.requires && t.requires.length > 0 && (
                  <div className="absolute bottom-3 right-3 flex flex-wrap items-center justify-end gap-1">
                    {t.requires.slice(0, 3).map((r) => (
                      <span
                        key={r}
                        className={`rounded-full border px-2 py-0.5 text-[10px] ${runtimePillClass(r)}`}
                        title={`May require: ${r}`}
                      >
                        {r}
                      </span>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Advanced: paste SKILL.md */}
      <div className="space-y-3">
        <button
          type="button"
          onClick={() => setAdvancedOpen((v) => !v)}
          className="flex w-full items-center justify-between rounded-lg border border-border bg-surface-elevated/50 p-3 text-left"
        >
          <span className="text-sm font-medium text-text">Advanced: paste SKILL.md</span>
          <span className="text-xs text-text-subtle">{advancedOpen ? "Hide" : "Show"}</span>
        </button>

        {advancedOpen && (
          <div className="space-y-3">
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              placeholder="Paste SKILL.md content here..."
              rows={10}
              className="w-full rounded-lg border border-border bg-surface p-3 font-mono text-sm text-text placeholder:text-text-subtle focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
            />

            <div className="flex items-center justify-between">
              <Button variant="ghost" onClick={handleCreateBlank} disabled={saving}>
                Create Blank
              </Button>
              <Button onClick={handleSave} disabled={!content.trim() || saving}>
                {saving ? "Saving..." : "Save"}
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* Installed skills */}
      <div ref={installedRef} className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-medium text-text">
            Installed skills
            {queryLower ? ` (${filteredSkills.length} of ${skills.length})` : ` (${skills.length})`}
          </h3>
        </div>

        {filteredSkills.length === 0 ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-6 text-center">
            <p className="text-sm text-text-muted">
              {skills.length === 0
                ? "No installed skills detected (folder or global)."
                : "No installed skills match your search."}
            </p>
            {skills.length === 0 && (
              <p className="mt-2 text-xs text-text-subtle">
                Install a starter skill above, or paste a SKILL.md in Advanced. You can remove
                skills later with the trash icon.
              </p>
            )}
          </div>
        ) : (
          <div className="space-y-3">
            {projectSkills.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs font-medium text-text-subtle">Folder Skills</p>
                {projectSkills.map((skill) => (
                  <SkillCard key={skill.path} skill={skill} onDelete={onRefresh} />
                ))}
              </div>
            )}

            {globalSkills.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs font-medium text-text-subtle">Global Skills</p>
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
          Tandem automatically uses installed skills when relevant - no selection needed.
        </p>
        <div className="text-text-muted">
          <p className="font-medium">Find skills to copy:</p>
          <ul className="ml-4 mt-1 list-disc space-y-1 text-xs">
            <li>
              <button
                onClick={() => openUrl("https://skillhub.club")}
                className="cursor-pointer text-primary hover:underline"
              >
                SkillHub
              </button>{" "}
              - 7,000+ community skills
            </li>
            <li>
              <button
                onClick={() => openUrl("https://github.com/search?q=SKILL.md&type=code")}
                className="cursor-pointer text-primary hover:underline"
              >
                GitHub
              </button>{" "}
              - Search &quot;SKILL.md&quot;
            </li>
          </ul>
        </div>
      </div>
    </div>
  );
}
