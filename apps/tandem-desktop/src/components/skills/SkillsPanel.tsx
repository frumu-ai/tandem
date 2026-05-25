import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { SkillCard } from "./SkillCard";
import {
  importSkill,
  installSkillTemplate,
  listSkillTemplates,
  skillsImport,
  skillsImportPreview,
  type SkillsConflictPolicy,
  type SkillsImportPreview,
  type SkillInfo,
  type SkillLocation,
  type SkillTemplateInfo,
} from "@/lib/tauri";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
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
  const { t } = useTranslation(["skills", "common"]);
  const canonicalMarketingTemplateIds = useMemo(
    () =>
      new Set([
        "product-marketing-context",
        "content-strategy",
        "seo-audit",
        "social-content",
        "copywriting",
        "copy-editing",
        "email-sequence",
        "competitor-alternatives",
        "launch-strategy",
      ]),
    []
  );
  const legacyMarketingTemplateIds = useMemo(
    () =>
      new Set([
        "marketing-content-creation",
        "marketing-campaign-planning",
        "marketing-brand-voice",
        "marketing-competitive-analysis",
        "marketing-research-posting-plan",
      ]),
    []
  );
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
  const [importPath, setImportPath] = useState("");
  const [importNamespace, setImportNamespace] = useState("");
  const [conflictPolicy, setConflictPolicy] = useState<SkillsConflictPolicy>("skip");
  const [importPreview, setImportPreview] = useState<SkillsImportPreview | null>(null);
  const [importingPack, setImportingPack] = useState(false);

  // Extract project name from path for display
  const projectName = projectPath
    ? projectPath.split(/[\\/]/).pop() || t("navigation.activeFolder", { ns: "common" })
    : null;
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
      setError(t("skills:errors.pasteSkillContent"));
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
      setError(err instanceof Error ? err.message : t("import.error"));
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
      setError(err instanceof Error ? err.message : t("templates.installFailed"));
    } finally {
      setInstallingTemplateId(null);
    }
  };

  const handleChooseImportPath = async () => {
    try {
      const picked = await openDialog({
        title: t("import.selectFileDialogTitle"),
        multiple: false,
        filters: [
          { name: "Skill Files", extensions: ["md", "zip"] },
          { name: "All Files", extensions: ["*"] },
        ],
      });
      if (typeof picked === "string") {
        setImportPath(picked);
      }
    } catch (e) {
      console.error("Failed to select import file:", e);
    }
  };

  const handlePreviewImport = async () => {
    if (!importPath.trim()) {
      setError(t("skills:errors.chooseImportFirst"));
      return;
    }
    try {
      setError(null);
      const preview = await skillsImportPreview(
        importPath,
        location,
        importNamespace.trim() || undefined,
        conflictPolicy
      );
      setImportPreview(preview);
    } catch (e) {
      setError(e instanceof Error ? e.message : t("skills:errors.previewImportFailed"));
      setImportPreview(null);
    }
  };

  const handleApplyImport = async () => {
    if (!importPath.trim()) return;
    try {
      setImportingPack(true);
      setError(null);
      const result = await skillsImport(
        importPath,
        location,
        importNamespace.trim() || undefined,
        conflictPolicy
      );
      if (result.errors.length > 0) {
        setError(`Imported with errors: ${result.errors.slice(0, 2).join(" | ")}`);
      }
      await onRefresh();
      if (onRestartSidecar) await onRestartSidecar();
      setImportPreview(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : t("skills:errors.importPackFailed"));
    } finally {
      setImportingPack(false);
    }
  };

  const queryLower = query.trim().toLowerCase();

  const filteredTemplates = useMemo(() => {
    const filtered = templates.filter((t) => {
      if (!queryLower) return true;
      const hay = `${t.name} ${t.description}`.toLowerCase();
      return hay.includes(queryLower);
    });
    const marketingIntent =
      !queryLower ||
      /(marketing|seo|social|linkedin|twitter|x |email|drip|copy|launch|competitor|alternative|content)/.test(
        queryLower
      );
    if (!marketingIntent) return filtered;

    const rank = (id: string) => {
      if (canonicalMarketingTemplateIds.has(id)) return 0;
      if (legacyMarketingTemplateIds.has(id)) return 2;
      return 1;
    };

    return [...filtered].sort((a, b) => {
      const r = rank(a.id) - rank(b.id);
      if (r !== 0) return r;
      return a.name.localeCompare(b.name);
    });
  }, [templates, queryLower, canonicalMarketingTemplateIds, legacyMarketingTemplateIds]);

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
        <span className="text-sm text-text-muted">{t("skills:location.saveTo")}</span>
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
                {t("skills:location.activeFolder")}:{" "}
                <span className="font-bold" style={{ color: "var(--color-primary)" }}>
                  {projectName}
                </span>
                <span className="ml-2 text-text-subtle text-xs">(.opencode/skill/)</span>
              </>
            ) : (
              t("skills:location.folderNotSelected")
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
          <span className="text-sm text-text">{t("skills:location.globalPathLabel")}</span>
        </label>
      </div>

      {/* Runtime note */}
      <div className="rounded-lg border border-border bg-surface-elevated/50 p-3 text-xs text-text-muted">
        <div className="flex items-center justify-between gap-3">
          <p className="font-medium text-text">{t("skills:runtime.title")}</p>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setShowPythonWizard(true)}
            className="h-7 px-2 text-[11px]"
          >
            {t("skills:runtime.setupPython")}
          </Button>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <span className="text-[11px] text-text-subtle">{t("skills:runtime.requires")}</span>
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
        <p className="mt-1">{t("skills:runtime.body")}</p>
      </div>
      {showPythonWizard && <PythonSetupWizard onClose={() => setShowPythonWizard(false)} />}

      {/* Search */}
      <div className="max-w-md">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("skills:search.placeholder")}
        />
      </div>

      {/* Installed skills summary */}
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-surface-elevated/50 p-3">
        <div className="min-w-0">
          <p className="text-sm font-medium text-text">{t("skills:installed.summaryTitle")}</p>
          <p className="mt-0.5 text-xs text-text-muted">
            {t("skills:installed.summaryCounts", {
              projectCount: allProjectSkills.length,
              globalCount: allGlobalSkills.length,
            })}
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
          {t("skills:installed.jump")}
        </Button>
      </div>

      {/* Starter templates */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <label className="text-sm font-medium text-text">
            Starter skills
            {queryLower ? ` (${filteredTemplates.length} of ${templates.length})` : ""}
          </label>
          <span className="text-xs text-text-subtle">{t("skills:templates.quickAdds")}</span>
        </div>

        {templatesLoading ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
            {t("skills:templates.loading")}
          </div>
        ) : filteredTemplates.length === 0 ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
            {templates.length === 0
              ? t("skills:templates.empty")
              : t("skills:templates.emptySearch")}
          </div>
        ) : (
          <div className="grid gap-3 md:grid-cols-2">
            {filteredTemplates.map((template) => (
              <div
                key={template.id}
                className="relative rounded-lg border border-border bg-surface-elevated p-4 pb-10"
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate text-sm font-semibold text-text">{template.name}</p>
                    <p className="mt-1 text-xs text-text-muted">{template.description}</p>
                  </div>
                  <Button
                    size="sm"
                    onClick={() => handleInstallTemplate(template.id)}
                    disabled={!!installingTemplateId}
                  >
                    {installingTemplateId === template.id
                      ? t("skills:templates.installing")
                      : t("skills:templates.install")}
                  </Button>
                </div>

                {template.requires && template.requires.length > 0 && (
                  <div className="absolute bottom-3 right-3 flex flex-wrap items-center justify-end gap-1">
                    {template.requires.slice(0, 3).map((r) => (
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
          <span className="text-sm font-medium text-text">{t("skills:advanced.title")}</span>
          <span className="text-xs text-text-subtle">
            {advancedOpen ? t("skills:advanced.hide") : t("skills:advanced.show")}
          </span>
        </button>

        {advancedOpen && (
          <div className="space-y-3">
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              placeholder={t("skills:advanced.placeholder")}
              rows={10}
              className="w-full rounded-lg border border-border bg-surface p-3 font-mono text-sm text-text placeholder:text-text-subtle focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
            />

            <div className="flex items-center justify-between">
              <Button variant="ghost" onClick={handleCreateBlank} disabled={saving}>
                {t("skills:advanced.createBlank")}
              </Button>
              <Button onClick={handleSave} disabled={!content.trim() || saving}>
                {saving ? t("skills:advanced.saving") : t("common:actions.save")}
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className="space-y-3 rounded-lg border border-border bg-surface-elevated/50 p-4">
        <div className="flex items-center justify-between">
          <p className="text-sm font-medium text-text">{t("skills:import.fromFileZip")}</p>
          <span className="text-xs text-text-subtle">{t("skills:import.previewBeforeApply")}</span>
        </div>
        <div className="flex flex-wrap gap-2">
          <Input
            value={importPath}
            onChange={(e) => setImportPath(e.target.value)}
            placeholder={t("skills:import.pathPlaceholder")}
          />
          <Button variant="secondary" onClick={handleChooseImportPath}>
            {t("common:actions.browse")}
          </Button>
        </div>
        <div className="grid gap-2 md:grid-cols-3">
          <Input
            value={importNamespace}
            onChange={(e) => setImportNamespace(e.target.value)}
            placeholder={t("skills:import.namespacePlaceholder")}
          />
          <select
            value={conflictPolicy}
            onChange={(e) => setConflictPolicy(e.target.value as SkillsConflictPolicy)}
            className="rounded-md border border-border bg-surface px-3 py-2 text-sm text-text"
          >
            <option value="skip">{t("skills:import.conflicts.skip")}</option>
            <option value="overwrite">{t("skills:import.conflicts.overwrite")}</option>
            <option value="rename">{t("skills:import.conflicts.rename")}</option>
          </select>
          <Button onClick={handlePreviewImport}>{t("skills:import.previewButton")}</Button>
        </div>

        {importPreview && (
          <div className="rounded border border-border bg-surface p-3 text-sm">
            <p className="text-text">
              {importPreview.valid} valid / {importPreview.invalid} invalid /{" "}
              {importPreview.conflicts} conflicts
            </p>
            <div className="mt-2 space-y-1 text-xs">
              {importPreview.items.slice(0, 8).map((item, idx) => (
                <div key={`${item.source}-${idx}`} className="flex items-center gap-2">
                  <span className={item.valid ? "text-success" : "text-error"}>
                    {item.valid ? t("skills:import.ok") : t("skills:import.err")}
                  </span>
                  <span className="truncate text-text-muted">{item.source}</span>
                  {item.name && <span className="text-text">→ {item.name}</span>}
                  <span className="text-text-subtle">({item.action})</span>
                </div>
              ))}
            </div>
            <div className="mt-3">
              <Button onClick={handleApplyImport} disabled={importingPack}>
                {importingPack ? t("skills:import.importing") : t("skills:import.applyButton")}
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* Installed skills */}
      <div ref={installedRef} className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-medium text-text">
            {t("skills:installed.title")}
            {queryLower ? ` (${filteredSkills.length} of ${skills.length})` : ` (${skills.length})`}
          </h3>
        </div>

        {filteredSkills.length === 0 ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-6 text-center">
            <p className="text-sm text-text-muted">
              {skills.length === 0
                ? t("skills:installed.empty")
                : t("skills:installed.emptySearch")}
            </p>
            {skills.length === 0 && (
              <p className="mt-2 text-xs text-text-subtle">{t("skills:installed.emptyHint")}</p>
            )}
          </div>
        ) : (
          <div className="space-y-3">
            {projectSkills.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs font-medium text-text-subtle">
                  {t("skills:installed.folderSkills")}
                </p>
                {projectSkills.map((skill) => (
                  <SkillCard key={skill.path} skill={skill} onDelete={onRefresh} />
                ))}
              </div>
            )}

            {globalSkills.length > 0 && (
              <div className="space-y-2">
                <p className="text-xs font-medium text-text-subtle">
                  {t("skills:installed.globalSkills")}
                </p>
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
        <p className="text-text-muted">{t("skills:help.autoUse")}</p>
        <div className="rounded border border-border bg-surface p-3 text-xs text-text-muted">
          <p className="font-medium text-text">{t("skills:help.marketingTitle")}</p>
          <p className="mt-1">{t("skills:help.marketingPreferred")}</p>
          <p className="mt-1">{t("skills:help.marketingLegacy")}</p>
        </div>
        <div className="text-text-muted">
          <p className="font-medium">{t("skills:help.findSkillsToCopy")}</p>
          <ul className="ml-4 mt-1 list-disc space-y-1 text-xs">
            <li>
              <button
                onClick={() => openUrl("https://skillhub.club")}
                className="cursor-pointer text-primary hover:underline"
              >
                SkillHub
              </button>{" "}
              - {t("skills:help.skillhubBlurb")}
            </li>
            <li>
              <button
                onClick={() => openUrl("https://github.com/search?q=SKILL.md&type=code")}
                className="cursor-pointer text-primary hover:underline"
              >
                GitHub
              </button>{" "}
              - {t("skills:help.githubBlurb")}
            </li>
          </ul>
        </div>
      </div>
    </div>
  );
}
